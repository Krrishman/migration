//! Local user profile discovery notes. Profile enumeration itself lives in
//! the platform adapter; this module adds the policy-level warnings.

use super::*;
use crate::error::AppResult;

pub struct UserProfilesModule;

impl DiscoveryModule for UserProfilesModule {
    fn id(&self) -> &'static str {
        "users"
    }
    fn stage(&self) -> &'static str {
        "Checking user profiles"
    }
    fn discover(&self, ctx: &DiscoveryContext, users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()> {
        let system = users.iter().filter(|u| u.is_system_account).count();
        if system > 0 {
            acc.warnings.push(Warning::info(
                WarningCode::Skipped,
                format!("{system} built-in system profile(s) (SYSTEM, LocalService, NetworkService, service accounts) are not offered for capture."),
            ));
        }
        let missing: Vec<_> = users.iter().filter(|u| !u.is_system_account && !u.profile_exists).map(|u| u.account_name.clone()).collect();
        if !missing.is_empty() {
            acc.warnings.push(Warning::info(WarningCode::Skipped, format!("Profile folder missing for: {}", missing.join(", "))));
        }
        let others = eligible_users(users).filter(|u| !u.is_current_user).count();
        if others > 0 && !ctx.platform.is_elevated() {
            acc.warnings.push(Warning::info(
                WarningCode::AdminRequired,
                format!("{others} other user profile(s) found. Their folders are usually readable only when running elevated; inaccessible items are marked and skipped."),
            ));
        }
        Ok(())
    }
}
