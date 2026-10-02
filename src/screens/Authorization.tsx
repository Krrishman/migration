import { ShieldAlert } from "lucide-react";
import { useState } from "react";
import { Alert, Button, Checkbox, Dialog } from "../components/ui";
import { errorMessage } from "../lib/api";
import { useApp } from "../lib/context";

export function PrivacyContent() {
  return (
    <div className="space-y-3 text-[13px] leading-relaxed">
      <p>Migration Assistant works only on the computer it runs on. It makes no network connections, sends no telemetry and has no remote features.</p>
      <div>
        <p className="font-semibold">What it reads during a scan</p>
        <ul className="ml-5 list-disc text-muted">
          <li>Computer name, Windows version, hardware summary, drive sizes and network adapter names</li>
          <li>Local user profile list (account names, SIDs, profile paths, approximate last use)</li>
          <li>File and folder names and sizes in the user folders you review</li>
          <li>Browser profile names, Outlook signature/template/data-file locations and Outlook profile names</li>
          <li>Printer, mapped drive and installed application inventories</li>
        </ul>
      </div>
      <div>
        <p className="font-semibold">What it never collects</p>
        <ul className="ml-5 list-disc text-muted">
          <li>Passwords, saved browser logins, cookies, authentication tokens or session data</li>
          <li>Windows Credential Manager, DPAPI keys, Wi-Fi passwords, private keys or certificates</li>
          <li>License keys, program folders or installers</li>
        </ul>
      </div>
      <p>
        Data is copied only for the items you select, into the destination folder you choose. Detailed reports stay inside the bundle; a redacted summary report is provided for
        sharing. Recent-item history is privacy-sensitive and is never selected unless you enable it.
      </p>
    </div>
  );
}

export function AuthorizationDialog() {
  const { backend, refreshStatus, status } = useApp();
  const [checked, setChecked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <Dialog
      open
      onOpenChange={() => {
        /* acknowledgement is required; the dialog cannot be dismissed */
      }}
      dismissible={false}
      title="Authorized use only"
      description={`Migration Assistant ${status.app_version} on ${status.computer_name}`}
      wide
      footer={
        <Button
          variant="primary"
          disabled={!checked}
          busy={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await backend.acknowledgeAuthorization();
              await refreshStatus();
            } catch (e) {
              setError(errorMessage(e));
            } finally {
              setBusy(false);
            }
          }}
        >
          I understand and I am authorized
        </Button>
      }
    >
      <Alert tone="warn" icon={<ShieldAlert className="h-5 w-5 text-warn" />} title="Use this tool only on computers and accounts you are authorized to migrate.">
        It copies personal files and settings. Make sure the device owner and the affected users have approved the migration according to your organization's policy.
      </Alert>
      <PrivacyContent />
      <label className="flex items-start gap-3 rounded-lg border border-border p-3">
        <Checkbox label="I am authorized" checked={checked} onChange={(e) => setChecked(e.target.checked)} className="mt-0.5" />
        <span>I am authorized by the device owner and the affected users to scan this computer and copy the data I select.</span>
      </label>
      {error && <p className="text-danger">{error}</p>}
    </Dialog>
  );
}
