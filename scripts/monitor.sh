#!/usr/bin/env bash
# Forge9 ecosystem monitor — tracks MERGED PRs and significant activity.
# Zero AI tokens: pure gh CLI + jq/python3 diffing.
set -euo pipefail

STATE_DIR="${HOME}/.cache/forge9-monitor"
mkdir -p "$STATE_DIR"
REPORT_FILE="${STATE_DIR}/latest_report.txt"
> "$REPORT_FILE"

REPOS=(
  "Marvisak/VIctory"
  "ivall/xcl"
  "omonkeyz/xbox-emulator"
  "SentinelInteractive/0x52504639"
  "Ahmet-Dedeler/rage9-re"
  "breadbbbb/rpf9-explorer"
  "Ahmet-Dedeler/bcpack-rs"
  "JoshRob297/rpf9-research"
  "Freminet/ScarlettX"
  "brcly/TuxBox"
  "zeozeozeo/rpf9"
  "pragyaangaur/VIctory-macOS"
)

KEYWORDS="encrypt|decrypt|key|cipher|AES|RPF9|repack|RTS0|mesh.shader|root.signature|mod.loader|DMCA|takedown|BCPack|BC6H|BC7|repacking|write.*archive|native.*texture"

merged_count=0
new_issues_count=0
hot_count=0

for repo in "${REPOS[@]}"; do
  slug="${repo//\//_}"

  # === 1. MERGED PRs (the real signal) ===
  merged_file="${STATE_DIR}/${slug}_merged.json"
  merged_now=$(gh pr list -R "$repo" --state merged --limit 10 --json number,title,mergedAt,headRefName 2>/dev/null || echo "[]")

  if [[ -f "$merged_file" ]]; then
    old_numbers=$(python3 -c "import json;print(' '.join(str(x['number']) for x in json.loads(open('$merged_file').read())))" 2>/dev/null || echo "")
    new_merges=$(echo "$merged_now" | python3 -c "
import json,sys
old = set('$old_numbers'.split())
for pr in json.load(sys.stdin):
    if str(pr['number']) not in old:
        print(f\"  MERGED PR #{pr['number']}: {pr['title']} ({pr['headRefName']})\")
" 2>/dev/null || true)
    if [[ -n "$new_merges" ]]; then
      merged_count=$((merged_count + 1))
      echo "[$repo] NEW MERGES:" >> "$REPORT_FILE"
      echo "$new_merges" >> "$REPORT_FILE"
      echo "" >> "$REPORT_FILE"
    fi
  fi
  echo "$merged_now" > "$merged_file"

  # === 2. Recent commits to default branch (last 1h) ===
  commits_file="${STATE_DIR}/${slug}_commits.txt"
  recent_commits=$(gh api "repos/$repo/commits?per_page=5" --jq '.[].sha' 2>/dev/null || echo "")

  if [[ -f "$commits_file" ]]; then
    old_commits=$(cat "$commits_file")
    new_shas=$(comm -23 <(echo "$recent_commits" | sort) <(echo "$old_commits" | sort) 2>/dev/null || true)
    new_count=$(echo "$new_shas" | grep -c '[a-f0-9]' || true)
    if [[ "$new_count" -gt 0 ]]; then
      echo "[$repo] $new_count new commit(s) on default branch" >> "$REPORT_FILE"
    fi
  fi
  echo "$recent_commits" > "$commits_file"

  # === 3. Hot issues (keyword match, new only) ===
  issues_file="${STATE_DIR}/${slug}_issues.json"
  issues_now=$(gh issue list -R "$repo" --limit 5 --state all --json number,title 2>/dev/null || echo "[]")

  if [[ -f "$issues_file" ]]; then
    old_issue_nums=$(python3 -c "import json;print(' '.join(str(x['number']) for x in json.loads(open('$issues_file').read())))" 2>/dev/null || echo "")
    hot_new=$(echo "$issues_now" | python3 -c "
import json,sys,re
old = set('$old_issue_nums'.split())
kw = re.compile(r'$KEYWORDS', re.I)
for i in json.load(sys.stdin):
    if str(i['number']) not in old and kw.search(i['title']):
        print(f\"  HOT issue #{i['number']}: {i['title']}\")
" 2>/dev/null || true)
    if [[ -n "$hot_new" ]]; then
      hot_count=$((hot_count + 1))
      echo "[$repo] KEYWORD ALERTS:" >> "$REPORT_FILE"
      echo "$hot_new" >> "$REPORT_FILE"
      echo "" >> "$REPORT_FILE"
    fi
  fi
  echo "$issues_now" > "$issues_file"
done

# === Our GitHub notifications + replies to our threads ===
# Uses the nineforge0 token file if present.
TOKEN_FILE="/Users/alexanderortiz/.t3/userdata/secrets/files/6048ce23b2ce05357ae640c803d5aeab"
if [[ -f "$TOKEN_FILE" ]]; then
  GH_TOKEN=$(cat "$TOKEN_FILE")
  notifs=$(curl -s -H "Authorization: token $GH_TOKEN" "https://api.github.com/notifications?per_page=30" 2>/dev/null || echo "[]")
  count=$(echo "$notifs" | python3 -c "import json,sys; d=json.load(sys.stdin); print(len(d) if isinstance(d,list) else 0)" 2>/dev/null || echo 0)
  if [[ "$count" -gt 0 ]]; then
    echo "=== GITHUB NOTIFICATIONS ($count unread) ===" >> "$REPORT_FILE"
    echo "$notifs" | python3 -c "
import json,sys
for n in json.load(sys.stdin):
    print(f\"  [{n['reason']}] {n['repository']['full_name']}: {n['subject']['title']}\")
" >> "$REPORT_FILE" 2>/dev/null || true
    echo "" >> "$REPORT_FILE"
    hot_count=$((hot_count + 1))
  fi

  # Track replies on threads we contributed to
  for pr in "zeozeozeo/rpf9/2" "Ahmet-Dedeler/awesome-gta6-reverse-engineering/17" "omonkeyz/xbox-emulator/142" "Marvisak/VIctory/614" "ivall/xcl/878" "Ahmet-Dedeler/bcpack-rs/4" "ivall/xcl/772" "Ahmet-Dedeler/bcpack-rs/5"; do
    repo="${pr%/*}"; num="${pr##*/}"
    pr_file="${STATE_DIR}/thread_${repo//\//_}_${num}.count"
    cur=$(curl -s -H "Authorization: token $GH_TOKEN" "https://api.github.com/repos/$repo/issues/$num/comments?per_page=100" 2>/dev/null | python3 -c "import json,sys; d=json.load(sys.stdin); print(len(d) if isinstance(d,list) else 0)" 2>/dev/null || echo 0)
    if [[ -f "$pr_file" ]]; then
      old=$(cat "$pr_file")
      if [[ "$cur" -gt "$old" ]]; then
        echo "[$repo#$num] $((cur-old)) new comment(s) on a thread we contributed to" >> "$REPORT_FILE"
        hot_count=$((hot_count + 1))
      fi
    fi
    echo "$cur" > "$pr_file"
  done
fi

# === Summary line ===
total=$((merged_count + hot_count))
if [[ $total -gt 0 ]]; then
  echo "=== FORGE9: $merged_count repos with new merges, $hot_count hot items ==="
  cat "$REPORT_FILE"
  exit 0
else
  echo "no significant changes"
  exit 1
fi
