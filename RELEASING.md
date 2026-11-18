# Feature & Release Workflow

cdj-media uses git flow with a multi-agent workflow for both feature development and releases.

## Branch Model

- `main` — released code only, always tagged
- `develop` — integration branch, features merge here
- `feature/*` — individual features, branch from develop
- `release/*` — release prep, branch from develop
- `hotfix/*` — urgent fixes, branch from main

## Versioning (semver)

- `feat` commits → minor bump (0.4.0 → 0.5.0)
- `fix` / `chore` commits → patch bump (0.5.0 → 0.5.1)
- Breaking change (`!`) → major bump (0.5.0 → 1.0.0)

## Feature Workflow

1. **product-owner** — prioritize and scope the feature on the roadmap
2. **release-manager** — "Start feature <name>"
3. **architect** — design the feature; produce a task breakdown for minions
4. **code-minions** — implement in parallel, one task each (failing test, implement, run checks, report)
5. **architect** — review all minion output; COMMIT when approved, otherwise dispatch fixes
6. **commit** — commit the approved changes
7. **release-manager** — "Finish feature <name>"

## Release Checklist

1. **architect** — review all changes since last release
2. **product-owner** — confirm the release delivers intended value
3. **documenter** — update README files to reflect the release
4. **release-manager** — "Start release 0.x.0" → confirm → "Finish release 0.x.0"
5. **Human** — push to remote
   ```
   git push origin main develop --tags
   ```

## Hotfix Checklist

1. **release-manager** — start hotfix
2. **commit** — commit the fix
3. **release-manager** — finish hotfix (confirm before calling)
4. **Human** — push

## Notes

- Agents never push — that always stays with the human
- Always confirm with release-manager before finishing a release or hotfix
- Multiple code-minions can run in parallel on different tasks within the same feature
- The commit agent invokes the conventional-commits skill for commit message format
- The architect never writes code — it designs and reviews only
- Never commit audio files; fixtures are rekordbox-exported metadata and analysis output only
