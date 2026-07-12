\set ON_ERROR_STOP on

-- Required psql variables:
--   database_name, migration_role, runtime_role
-- This script intentionally creates login roles without passwords. Set each password
-- interactively with psql's \password command after provisioning.

\if :{?database_name}
\else
  \echo 'database_name is required'
  \quit 3
\endif

\if :{?migration_role}
\else
  \echo 'migration_role is required'
  \quit 3
\endif

\if :{?runtime_role}
\else
  \echo 'runtime_role is required'
  \quit 3
\endif

SELECT (:'migration_role' <> :'runtime_role') AS roles_are_distinct
\gset
\if :roles_are_distinct
\else
  \echo 'migration_role and runtime_role must be different'
  DO $guard$
  BEGIN
      RAISE EXCEPTION 'migration_role and runtime_role must be different';
  END
  $guard$;
\endif

SELECT format(
    'CREATE ROLE %I LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION PASSWORD NULL',
    :'migration_role'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'migration_role')
\gexec

SELECT format(
    'ALTER ROLE %I WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION',
    :'migration_role'
)
\gexec

SELECT format(
    'CREATE ROLE %I LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION PASSWORD NULL',
    :'runtime_role'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'runtime_role')
\gexec

SELECT format(
    'ALTER ROLE %I WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION',
    :'runtime_role'
)
\gexec

SELECT format('CREATE DATABASE %I OWNER %I', :'database_name', :'migration_role')
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = :'database_name')
\gexec

SELECT format('ALTER DATABASE %I OWNER TO %I', :'database_name', :'migration_role')
\gexec
SELECT format('REVOKE ALL PRIVILEGES ON DATABASE %I FROM PUBLIC', :'database_name')
\gexec
SELECT format('GRANT CONNECT ON DATABASE %I TO %I', :'database_name', :'migration_role')
\gexec
SELECT format('GRANT CONNECT ON DATABASE %I TO %I', :'database_name', :'runtime_role')
\gexec
