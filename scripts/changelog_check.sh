#!/bin/bash

# Simply check if diff in changelog exists.
git diff --exit-code --quiet origin/master... ./CHANGELOG.md
diff_status=$?

if [ "$diff_status" -eq 0 ]; then
  echo "Following files have been changed:"
  git diff --name-only "origin/master..." ./
  echo ""
  echo "Please add a changelog entry in CHANGELOG.md or add 'skip changelog' label to your PR if this change does not require an entry."
  exit 1
fi

if [ "$diff_status" -ne 1 ]; then
  exit "$diff_status"
fi
