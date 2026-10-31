\set ON_ERROR_STOP on

-- Run this while connected as the migration role after all core migrations.
-- Required psql variable: runtime_role
-- Optional psql variable: service_admin_reset_enabled (default: false; development only)

\if :{?runtime_role}
\else
  \echo 'runtime_role is required'
  \quit 3
\endif

\if :{?service_admin_reset_enabled}
\else
  \set service_admin_reset_enabled false
\endif

BEGIN;

REVOKE ALL PRIVILEGES ON SCHEMA public FROM PUBLIC;
REVOKE CREATE ON SCHEMA public FROM :"runtime_role";
GRANT USAGE ON SCHEMA public TO :"runtime_role";

REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM :"runtime_role";
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO :"runtime_role";
REVOKE TRUNCATE, REFERENCES, TRIGGER ON ALL TABLES IN SCHEMA public FROM :"runtime_role";

-- SQLx-only databases have just the legacy ledger. An Alembic handover adds
-- both metadata tables; a fresh Alembic bootstrap has no SQLx ledger.
SELECT format('GRANT SELECT ON TABLE public.%I TO %I', metadata_table, :'runtime_role')
FROM (VALUES ('_sqlx_migrations'), ('alembic_version'), ('migration_provenance'))
    AS metadata(metadata_table)
WHERE to_regclass(format('public.%I', metadata_table)) IS NOT NULL
\gexec

SELECT format(
    'REVOKE INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER ON TABLE public.%I FROM %I',
    metadata_table, :'runtime_role'
)
FROM (VALUES ('_sqlx_migrations'), ('alembic_version'), ('migration_provenance'))
    AS metadata(metadata_table)
WHERE to_regclass(format('public.%I', metadata_table)) IS NOT NULL
\gexec

-- These tables are append-only ledgers. Future append-only tables must be added here
-- in the same release that creates them.
REVOKE UPDATE, DELETE, TRUNCATE
    ON TABLE public.auction_actions,
             public.auction_point_ledger,
             public.mmr_ledger_entries,
             public.admin_audit_log,
             public.dummy_user_audit_log
    FROM :"runtime_role";

-- Direct ledger/audit deletion remains denied. The guarded non-production service-admin reset
-- route receives only this narrow, guild-scoped capability.
REVOKE ALL PRIVILEGES
    ON FUNCTION public.service_admin_reset_guild_records(text)
    FROM PUBLIC;
REVOKE ALL PRIVILEGES
    ON FUNCTION public.service_admin_reset_guild_records(text)
    FROM :"runtime_role";
\if :service_admin_reset_enabled
  GRANT EXECUTE
      ON FUNCTION public.service_admin_reset_guild_records(text)
      TO :"runtime_role";
\endif

ALTER DEFAULT PRIVILEGES IN SCHEMA public
    REVOKE ALL PRIVILEGES ON TABLES FROM :"runtime_role";
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO :"runtime_role";

COMMIT;
