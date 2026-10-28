#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
command -v docker >/dev/null || { printf '%s\n' 'Install Docker Engine and Compose first.'; exit 1; }
[ -f .env ] || { printf '%s\n' 'Create .env with PUBLIC_URL=https://your-host and DOMAIN=your-host.'; exit 1; }
[ -f data/accounts.json ] || { printf '%s\n' 'Import your accounts privately into data/accounts.json first.'; exit 1; }

# WP-15: Enforce pre-provisioned single-owner bootstrap before public exposure
[ -f data/console/access.json ] || { printf '%s\n' 'Owner account must be pre-configured privately in data/console/access.json before starting production stack.'; exit 1; }

docker compose config --quiet
docker compose build
# Initialize only this project's dedicated data mount before the non-root app starts.
docker compose run --rm --no-deps data-init
docker compose up -d
docker compose ps
printf '%s\n' 'Production gateway running with HTTPS. Console accessible at /console/'
