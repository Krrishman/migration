//! Parsing and classification for the PowerShell printer adapter. Kept free
//! of Windows APIs so it is unit-tested on every OS.

use crate::models::{PrinterConnection, PrinterInfo};
use serde::Deserialize;

/// Read-only inventory script. Uses only built-in PrintManagement cmdlets and
/// emits a single compressed JSON object. Values are stringified so enum
/// serialization differences between PowerShell versions do not matter.
pub const INVENTORY_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
if (-not (Get-Command Get-Printer -ErrorAction SilentlyContinue)) { '{"unavailable":true}'; exit 0 }
$printers = @(Get-Printer | Select-Object @{n='Name';e={[string]$_.Name}}, @{n='ShareName';e={[string]$_.ShareName}},
  @{n='PortName';e={[string]$_.PortName}}, @{n='DriverName';e={[string]$_.DriverName}},
  @{n='Type';e={[string]$_.Type}}, @{n='ComputerName';e={[string]$_.ComputerName}},
  @{n='PrinterStatus';e={[string]$_.PrinterStatus}})
$ports = @(Get-PrinterPort | Select-Object @{n='Name';e={[string]$_.Name}}, @{n='Description';e={[string]$_.Description}},
  @{n='PrinterHostAddress';e={[string]$_.PrinterHostAddress}}, @{n='PortMonitor';e={[string]$_.PortMonitor}})
$drivers = @(Get-PrinterDriver | Select-Object @{n='Name';e={[string]$_.Name}}, @{n='DriverVersion';e={
  $v = [uint64]$_.DriverVersion; '{0}.{1}.{2}.{3}' -f (($v -shr 48) -band 0xffff), (($v -shr 32) -band 0xffff), (($v -shr 16) -band 0xffff), ($v -band 0xffff) }})
$default = $null
try { $default = [string](Get-CimInstance -ClassName Win32_Printer -Filter 'Default=TRUE' | Select-Object -First 1).Name } catch { }
[pscustomobject]@{ printers = $printers; ports = $ports; drivers = $drivers; default = $default } | ConvertTo-Json -Depth 4 -Compress
"#;

/// Restore scripts read every value from environment variables so no data
/// is ever interpolated into PowerShell source (no injection surface).
pub const CONNECT_SHARED_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Printer -ConnectionName $env:MA_UNC
"#;

pub const ADD_NETWORK_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
if (-not (Get-PrinterDriver -Name $env:MA_DRIVER -ErrorAction SilentlyContinue)) { throw "Printer driver is not installed: $($env:MA_DRIVER)" }
if (-not (Get-PrinterPort -Name $env:MA_PORT -ErrorAction SilentlyContinue)) { Add-PrinterPort -Name $env:MA_PORT -PrinterHostAddress $env:MA_HOST }
if (Get-Printer -Name $env:MA_NAME -ErrorAction SilentlyContinue) { throw "A printer with this name already exists: $($env:MA_NAME)" }
Add-Printer -Name $env:MA_NAME -DriverName $env:MA_DRIVER -PortName $env:MA_PORT
"#;

pub const DRIVER_EXISTS_SCRIPT: &str = r#"
if (Get-PrinterDriver -Name $env:MA_DRIVER -ErrorAction SilentlyContinue) { 'yes' } else { 'no' }
"#;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawPrinter {
    name: String,
    #[serde(default)]
    share_name: Option<String>,
    #[serde(default)]
    port_name: Option<String>,
    #[serde(default)]
    driver_name: Option<String>,
    #[serde(default, rename = "Type")]
    kind: Option<String>,
    #[serde(default)]
    computer_name: Option<String>,
    #[serde(default)]
    printer_status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawPort {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    printer_host_address: Option<String>,
    #[serde(default)]
    port_monitor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawDriver {
    name: String,
    #[serde(default)]
    driver_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawInventory {
    #[serde(default)]
    unavailable: bool,
    #[serde(default, deserialize_with = "one_or_many")]
    printers: Vec<RawPrinter>,
    #[serde(default, deserialize_with = "one_or_many")]
    ports: Vec<RawPort>,
    #[serde(default, deserialize_with = "one_or_many")]
    drivers: Vec<RawDriver>,
    #[serde(default)]
    default: Option<String>,
}

/// ConvertTo-Json collapses single-element arrays to objects; accept both.
fn one_or_many<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Null => Ok(vec![]),
        serde_json::Value::Array(a) => a.into_iter().map(|x| serde_json::from_value(x).map_err(serde::de::Error::custom)).collect(),
        other => Ok(vec![serde_json::from_value(other).map_err(serde::de::Error::custom)?]),
    }
}

#[derive(Debug)]
pub enum ParseOutcome {
    Unavailable,
    Printers(Vec<PrinterInfo>),
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}

pub fn classify(name: &str, kind: Option<&str>, port_name: &str, port: Option<(&str, &str, &str)>, driver: &str) -> PrinterConnection {
    let pn = port_name.to_ascii_uppercase();
    let d = driver.to_ascii_lowercase();
    if name.starts_with(r"\\") || kind.is_some_and(|k| k.eq_ignore_ascii_case("connection")) {
        return PrinterConnection::Shared;
    }
    if ["PORTPROMPT:", "NUL:", "FILE:", "SHRFAX:"].contains(&pn.as_str())
        || d.contains("print to pdf")
        || d.contains("xps document writer")
        || d.contains("onenote")
        || d.contains("fax")
    {
        return PrinterConnection::Virtual;
    }
    if pn.starts_with("USB") || pn.starts_with("DOT4") {
        return PrinterConnection::Usb;
    }
    if pn.starts_with("WSD") {
        return PrinterConnection::Wsd;
    }
    if let Some((_, monitor, host)) = port {
        if !host.is_empty() || monitor.to_ascii_lowercase().contains("tcpmon") {
            return PrinterConnection::Network;
        }
    }
    if pn.starts_with("IP_") || pn.contains('.') {
        return PrinterConnection::Network;
    }
    if pn.starts_with("LPT") || pn.starts_with("COM") {
        return PrinterConnection::Local;
    }
    PrinterConnection::Other
}

pub fn parse_inventory(json: &str) -> Result<ParseOutcome, String> {
    let raw: RawInventory = serde_json::from_str(json.trim()).map_err(|e| format!("unexpected printer adapter output: {e}"))?;
    if raw.unavailable {
        return Ok(ParseOutcome::Unavailable);
    }
    let default = non_empty(raw.default);
    let printers = raw
        .printers
        .into_iter()
        .map(|p| {
            let port_name = non_empty(p.port_name).unwrap_or_default();
            let driver_name = non_empty(p.driver_name).unwrap_or_default();
            let port = raw.ports.iter().find(|x| x.name.eq_ignore_ascii_case(&port_name));
            let host = port.and_then(|x| non_empty(x.printer_host_address.clone()));
            let port_tuple = port.map(|x| {
                (x.description.as_deref().unwrap_or(""), x.port_monitor.as_deref().unwrap_or(""), host.as_deref().unwrap_or(""))
            });
            let connection = classify(&p.name, p.kind.as_deref(), &port_name, port_tuple, &driver_name);
            let share_name = non_empty(p.share_name);
            let unc_path = if p.name.starts_with(r"\\") {
                Some(p.name.clone())
            } else {
                match (non_empty(p.computer_name), &share_name) {
                    (Some(c), Some(s)) if connection == PrinterConnection::Shared => Some(format!(r"\\{c}\{s}")),
                    _ => None,
                }
            };
            PrinterInfo {
                is_default: default.as_deref().is_some_and(|d| d.eq_ignore_ascii_case(&p.name)),
                driver_version: raw.drivers.iter().find(|d| d.name.eq_ignore_ascii_case(&driver_name)).and_then(|d| non_empty(d.driver_version.clone())),
                port_type: port.and_then(|x| non_empty(x.description.clone()).or_else(|| non_empty(x.port_monitor.clone()))),
                host_address: host,
                status: non_empty(p.printer_status).unwrap_or_else(|| "Unknown".into()),
                name: p.name,
                share_name,
                port_name,
                unc_path,
                connection,
                driver_name,
            }
        })
        .collect();
    Ok(ParseOutcome::Printers(printers))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"printers":[
      {"Name":"HP LaserJet 4th Floor","ShareName":"","PortName":"IP_10.0.4.25","DriverName":"HP Universal Printing PCL 6","Type":"Local","ComputerName":"","PrinterStatus":"Normal"},
      {"Name":"\\\\printsrv01\\Finance-Color","ShareName":"Finance-Color","PortName":"\\\\printsrv01\\Finance-Color","DriverName":"Xerox Global Print Driver PCL6","Type":"Connection","ComputerName":"printsrv01","PrinterStatus":"Normal"},
      {"Name":"Brother HL-L2350DW","ShareName":"","PortName":"USB001","DriverName":"Brother HL-L2350D series","Type":"Local","ComputerName":"","PrinterStatus":"Offline"},
      {"Name":"Microsoft Print to PDF","ShareName":"","PortName":"PORTPROMPT:","DriverName":"Microsoft Print To PDF","Type":"Local","ComputerName":"","PrinterStatus":"Normal"},
      {"Name":"Canon WSD","ShareName":"","PortName":"WSD-1234","DriverName":"Canon Generic","Type":"Local","ComputerName":"","PrinterStatus":"Normal"}],
     "ports":[{"Name":"IP_10.0.4.25","Description":"Standard TCP/IP Port","PrinterHostAddress":"10.0.4.25","PortMonitor":"TCPMON.DLL"}],
     "drivers":[{"Name":"HP Universal Printing PCL 6","DriverVersion":"61.250.1.24832"}],
     "default":"HP LaserJet 4th Floor"}"#;

    #[test]
    fn parses_and_classifies() {
        let ParseOutcome::Printers(p) = parse_inventory(SAMPLE).unwrap() else { panic!() };
        assert_eq!(p.len(), 5);
        assert_eq!(p[0].connection, PrinterConnection::Network);
        assert_eq!(p[0].host_address.as_deref(), Some("10.0.4.25"));
        assert_eq!(p[0].driver_version.as_deref(), Some("61.250.1.24832"));
        assert!(p[0].is_default);
        assert_eq!(p[1].connection, PrinterConnection::Shared);
        assert_eq!(p[1].unc_path.as_deref(), Some(r"\\printsrv01\Finance-Color"));
        assert_eq!(p[2].connection, PrinterConnection::Usb);
        assert_eq!(p[3].connection, PrinterConnection::Virtual);
        assert_eq!(p[4].connection, PrinterConnection::Wsd);
    }

    #[test]
    fn handles_single_object_and_unavailable() {
        let one = r#"{"printers":{"Name":"P","PortName":"LPT1:","DriverName":"Generic"},"ports":null,"drivers":null,"default":null}"#;
        let ParseOutcome::Printers(p) = parse_inventory(one).unwrap() else { panic!() };
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].connection, PrinterConnection::Local);
        assert!(matches!(parse_inventory(r#"{"unavailable":true}"#).unwrap(), ParseOutcome::Unavailable));
        assert!(parse_inventory("garbage").is_err());
    }

    #[test]
    fn restore_scripts_never_interpolate_values() {
        for s in [CONNECT_SHARED_SCRIPT, ADD_NETWORK_SCRIPT, DRIVER_EXISTS_SCRIPT] {
            assert!(s.contains("$env:MA_"));
            assert!(!s.contains("{{"));
        }
    }
}
