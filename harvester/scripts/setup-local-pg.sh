#!/usr/bin/env bash
# Native Postgres + pgvector for the harvester — NO Docker.
# Idempotent: safe to re-run. Needs sudo (installs a system package, makes a db).
set -euo pipefail

# pick the newest postgresql-NN-pgvector apt offers
PGVER="$(apt-cache search '^postgresql-[0-9]+-pgvector$' 2>/dev/null \
         | grep -oE 'postgresql-[0-9]+' | grep -oE '[0-9]+' | sort -n | tail -1)"
PGVER="${PGVER:-17}"
echo "[1/4] installing postgresql + postgresql-${PGVER}-pgvector ..."
sudo apt-get update -qq
sudo apt-get install -y postgresql "postgresql-${PGVER}-pgvector"

echo "[2/4] starting postgres ..."
sudo systemctl enable --now postgresql

echo "[3/4] creating role 'harvest' + database 'harvest' ..."
sudo -u postgres psql -v ON_ERROR_STOP=1 -c \
  "DO \$\$ BEGIN
     IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='harvest') THEN
       CREATE ROLE harvest LOGIN PASSWORD 'harvest' SUPERUSER;
     END IF;
   END \$\$;"
sudo -u postgres psql -tAc "SELECT 1 FROM pg_database WHERE datname='harvest'" | grep -q 1 \
  || sudo -u postgres createdb -O harvest harvest

echo "[4/4] enabling pgvector extension ..."
sudo -u postgres psql -d harvest -c "CREATE EXTENSION IF NOT EXISTS vector;"

echo
echo "done. Use:"
echo "  export DATABASE_URL=postgres://harvest:harvest@localhost:5432/harvest"
