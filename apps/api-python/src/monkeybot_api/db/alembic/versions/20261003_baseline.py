"""Bootstrap the exact SQLx 0001..0026 business schema without a fake SQLx ledger.

Revision ID: 20261003_baseline
Revises:
"""

from alembic import op
from sqlalchemy.util.concurrency import await_only

from monkeybot_api.db.migration_handover import (
    BASELINE_REVISION,
    MigrationValidationError,
    _create_provenance_table,
    _empty_business_database,
    _record_provenance,
    bootstrap_sql,
    validate_baseline_catalog,
)

revision = BASELINE_REVISION
down_revision = None
branch_labels = None
depends_on = None


def upgrade() -> None:
    bind = op.get_bind()
    if not _empty_business_database(bind):
        raise MigrationValidationError("baseline revision requires an empty business database")
    driver = bind.connection.driver_connection
    if driver is None:
        raise MigrationValidationError("Alembic connection has no PostgreSQL driver")
    for script in bootstrap_sql():
        # asyncpg's simple-query execute accepts each reviewed SQLx file as one
        # transaction-scoped batch, preserving PL/pgSQL dollar-quoted bodies.
        await_only(driver.execute(script))
    _create_provenance_table(bind)
    validate_baseline_catalog(bind)
    _record_provenance(bind, "alembic_bootstrap")


def downgrade() -> None:
    raise MigrationValidationError("baseline schema downgrade is intentionally unavailable")
