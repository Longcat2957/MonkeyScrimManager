pub mod auth;
pub mod bot_workflow;
pub mod champion_preferences;
pub mod context;
pub mod dummy_recruitment;
pub mod dummy_users;
pub mod guild_admin_members;
pub mod guild_admin_settings;
pub mod guild_audit_logs;
pub mod guild_data_transfer;
pub mod guild_leaderboard;
pub mod guild_record_profiles;
pub mod guild_seasons;
pub mod idempotency;
pub mod mmr;
pub mod notification_outbox;
pub mod outbox;
pub mod riot;
pub mod riot_records;
pub mod search_suggestions;
pub mod service_status;
pub mod user_profiles;
pub mod workflow;

use crate::domain::GuildId;

const GLOBAL_SCOPE_KEY: &str = "GLOBAL";

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TenantScope {
    Global,
    Guild(GuildId),
}

impl TenantScope {
    pub const fn global() -> Self {
        Self::Global
    }

    pub const fn guild(guild_id: GuildId) -> Self {
        Self::Guild(guild_id)
    }

    pub fn guild_id(&self) -> Option<&GuildId> {
        match self {
            Self::Global => None,
            Self::Guild(guild_id) => Some(guild_id),
        }
    }

    pub fn scope_key(&self) -> &str {
        self.guild_id().map_or(GLOBAL_SCOPE_KEY, GuildId::as_str)
    }

    pub(crate) fn database_guild_id(&self) -> Option<&str> {
        self.guild_id().map(GuildId::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_scope_derives_database_keys_without_free_form_input() {
        let guild_id = GuildId::parse("123456789012345678").expect("valid guild ID");
        let guild = TenantScope::guild(guild_id.clone());

        assert_eq!(TenantScope::global().scope_key(), "GLOBAL");
        assert_eq!(TenantScope::global().database_guild_id(), None);
        assert_eq!(guild.scope_key(), guild_id.as_str());
        assert_eq!(guild.database_guild_id(), Some(guild_id.as_str()));
    }
}
