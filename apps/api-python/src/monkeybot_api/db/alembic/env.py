"""Alembic environment; standalone alembic commands are deliberately disabled."""

from alembic import context
from sqlalchemy.engine import Connection

connection = context.config.attributes.get("connection")
if not isinstance(connection, Connection):
    raise RuntimeError("use monkeybot-migrate-python for fenced migration commands")

context.configure(connection=connection, target_metadata=None, transaction_per_migration=False)
with context.begin_transaction():
    context.run_migrations()
