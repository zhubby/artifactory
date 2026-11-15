# Tenant and project workspaces

Approved 2026-11-15. Deliver the tenant hall, tenant project home and project workspace with a searchable tenant/project top bar and one contextual sidebar. Keep readable slugs and project-local build numbers; remove obsolete page routes without adapters.

- [x] Persist account display names; implement profile updates and password changes with rate limiting, atomic credential checks and session revocation.
- [x] Append lifecycle schema, durable leased operations, admission fences, archive/restore, draining and resumable scoped deletion across Control, Source, Runtime, Telemetry and object storage. Preserve identifier tombstones and unrelated resources.
- [x] Build independent tenant hall, contextual tenant/project workspaces and management pages; move creation, builds, artifacts and releases under projects. Preserve request isolation, accessibility and unsaved edits.
- [x] Provide a persistent bottom-right account menu and profile/security/preferences overlay. Keep existing themes, translations, reduced motion and preference persistence.
- [ ] Validate real PostgreSQL/object-store lifecycle behavior, account races, three-replica takeover, browser navigation and visual matrices. Record actual evidence and CI limitations.
- [ ] Merge and push main; back up and migrate in a maintenance window, deploy immutable images only in namespace artifactory, retain three Control replicas, and verify live acceptance using temporary resources.

Lifecycle policy: owner manages tenant lifecycle; project admin manages project lifecycle. Archive/delete close admission and wait for accepted work, without automatically cancelling it. Archived history remains readable. Delete requires the exact slug and current password; waiting may be cancelled, purging may only be retried. Deletion inventories durable IDs/object keys before removing rows; services own their purge and shared objects survive remaining references. Restore of a tenant preserves individually archived projects. Backups retain their existing policy.

Personal policy: editable display name, immutable login username, generated avatar, current-password verification and confirmation of a new password. Successful password change revokes every account session. Language/theme/effects move inside the authenticated personal overlay; anonymous pages retain basic preferences. Account requests never depend on a tenant selection.
