\set ON_ERROR_STOP on

-- Run this while connected as the migration role after all core migrations.
-- Required psql variable: runtime_role

\if :{?runtime_role}
\else
  \echo 'runtime_role is required'
  \quit 3
\endif

BEGIN;

REVOKE ALL PRIVILEGES ON SCHEMA public FROM PUBLIC;
REVOKE CREATE ON SCHEMA public FROM :"runtime_role";
GRANT USAGE ON SCHEMA public TO :"runtime_role";

REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM :"runtime_role";
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO :"runtime_role";
REVOKE TRUNCATE, REFERENCES, TRIGGER ON ALL TABLES IN SCHEMA public FROM :"runtime_role";

GRANT SELECT ON TABLE public._sqlx_migrations TO :"runtime_role";
REVOKE INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER
    ON TABLE public._sqlx_migrations FROM :"runtime_role";

-- These tables are append-only ledgers. Future append-only tables must be added here
-- in the same release that creates them.
REVOKE UPDATE, DELETE, TRUNCATE
    ON TABLE public.auction_actions,
             public.auction_point_ledger,
             public.mmr_ledger_entries,
             public.admin_audit_log,
             public.dummy_user_audit_log
    FROM :"runtime_role";

ALTER DEFAULT PRIVILEGES IN SCHEMA public
    REVOKE ALL PRIVILEGES ON TABLES FROM :"runtime_role";
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO :"runtime_role";

COMMIT;
