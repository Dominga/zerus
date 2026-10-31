# Upstream PR from phase 42: commands for the owner

Run these only on the owner's word. Nothing below has been run.

- Branch: `upstream-pr-42` in `D:\Tools\Zerus-teams-v2-team-528aebfe-expert`,
  tip `0cded315edc083c04fd86e49b2c82ed27bb81a86`.
- It fast-forwards `WhiteWh:agent-teams-v2` from `a15a7ff6`, so no force push
  is needed.
- The pull request body is `docs/superpowers/plans/upstream-pr-42.md`, which
  starts with the title line. Neither file is committed to the branch.

## 1. Check (read-only)

```powershell
$repo = 'D:\Tools\Zerus-teams-v2-team-528aebfe-expert'
git -C $repo rev-parse upstream-pr-42
git ls-remote https://github.com/WhiteWh/zerus.git refs/heads/agent-teams-v2
git -C $repo merge-base --is-ancestor a15a7ff6 upstream-pr-42; "fast-forward: $($LASTEXITCODE -eq 0)"
git ls-remote https://github.com/ufna/zerus.git refs/heads/main
```

- The first command must print `0cded315edc083c04fd86e49b2c82ed27bb81a86`.
- The second must still print `a15a7ff6ed2ec1581fd4a7da04e3fc5d04b4b4f1`.
- The last one prints upstream main, which was `0aaac554` when this was
  written. If upstream has moved since, ask for the branch to be merged again
  first.

## 2. Push (fast-forward only)

```powershell
git -C $repo push fork upstream-pr-42:agent-teams-v2
```

## 3. Open the pull request

With the GitHub CLI:

```powershell
gh pr create --repo ufna/zerus --base main --head WhiteWh:agent-teams-v2 `
  --title "Agent teams and the WhiteWh fork's desktop work, through phase 42" `
  --body-file "$repo\docs\superpowers\plans\upstream-pr-42.md"
```

The same through the API:

```powershell
gh api repos/ufna/zerus/pulls -X POST `
  -f title="Agent teams and the WhiteWh fork's desktop work, through phase 42" `
  -f head="WhiteWh:agent-teams-v2" -f base=main `
  -F body=@"$repo\docs\superpowers\plans\upstream-pr-42.md"
```

Without `gh`, using a token with pull-request write access in
`$env:GITHUB_TOKEN`:

```powershell
$body = @{ title = "Agent teams and the WhiteWh fork's desktop work, through phase 42"
           head = 'WhiteWh:agent-teams-v2'; base = 'main'
           body = Get-Content -Raw "$repo\docs\superpowers\plans\upstream-pr-42.md" } | ConvertTo-Json
Invoke-RestMethod -Method Post -Uri https://api.github.com/repos/ufna/zerus/pulls `
  -Headers @{ Authorization = "Bearer $env:GITHUB_TOKEN"; Accept = 'application/vnd.github+json' } `
  -Body $body -ContentType 'application/json'
```

GitHub listed no pull request with head `WhiteWh:agent-teams-v2` on
2026-10-30. If one exists by then, update its description instead:

```powershell
gh pr edit NUMBER --repo ufna/zerus --body-file "$repo\docs\superpowers\plans\upstream-pr-42.md"
```

## 4. Optional: close the pull requests this one replaces

This is the owner's choice: #322, #323 and #326 carry earlier parts of this
work.

```powershell
foreach ($n in 322, 323, 326) { gh pr close $n --repo ufna/zerus --comment "Superseded by #NEW." }
```
