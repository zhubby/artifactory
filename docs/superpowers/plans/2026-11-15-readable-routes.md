# Readable tenant, project and build routes

Approved scope: switch the development console directly to `/tenants/{tenant-slug}`, `/tenants/{tenant-slug}/projects/{project-slug}`, `/tenants/{tenant-slug}/projects/{project-slug}/builds/{number}` and tenant settings below the same tenant prefix. Do not retain `/t/{uuid}`, tenant/project/build UUID page routes, redirects or dual-route adapters. Internal UUIDs, device identities and authorization remain authoritative.

Tenant slugs are globally unique; project slugs are unique within a tenant. Generate lowercase ASCII slugs from the creation name, with deterministic collision suffixes and `tenant`/`project` fallbacks. Store them independently of display names and make them immutable. Build numbers start at one per project and are allocated transactionally by PostgreSQL; never reuse an issued number after deletion.

The browser resolves the tenant slug through an authenticated membership lookup before sending the existing UUID tenant header. Resolve project/build page identifiers through authorized tenant-scoped endpoints. Capture tenant context across awaits and ignore or abort stale responses. Explicit Next routes reject obsolete and malformed paths with 404.

## Implementation

- [x] Add failing database/API regressions for stable unique slugs, membership/project isolation, creation metadata, encoded identifiers and concurrent project-local build numbers.
- [x] Append the SQL migration and projection snapshot; expose slugs/numbers in typed responses and authorized lookup endpoints. Keep UUID resource APIs and their authorization checks.
- [x] Add failing browser regressions for readable navigation, deep links, old-path 404 and delayed tenant-resolution isolation.
- [x] Replace the catch-all route, tenant resolution and all project/build navigation. Update fixtures and active documentation without compatibility branches.
- [x] Run database and browser regressions, format, lint, type checks and production build; review permissions, races and migration behavior.
- [x] Commit scoped changes, merge and push main, build immutable deployment images, migrate and upgrade only namespace `artifactory`, preserving three control replicas, PVCs and identities. Verify live route/API health and Git/deployment revisions.

Delivery evidence and CI limitations: [Kubernetes upgrade receipt](../../releases/2026-11-15-readable-routes-kubernetes.md).

## Validation boundaries

Use the existing isolated local PostgreSQL database for schema/API tests, then browser fixtures plus a real Control/Web preview. Before Kubernetes changes, capture the current Helm release, images, health and identities and create a database backup. Verify rollout readiness, login, readable deep links and tenant isolation against the deployed application. Keep unrelated resources and the Gateway StatefulSet unchanged. Record any pre-existing CI environment failures separately from new regressions.
