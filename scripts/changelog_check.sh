#!/bin/bash

# Simply check if diff in changelog exists.
if git diff --exit-code --quiet origin/master... ./CHANGELOG.md; then
  echo "Following files have been changed:"
  echo "$(git diff --name-only "origin/master..." ./)"
  echo ""
  echo "Please add a changelog entry in CHANGELOG.md or add 'skip changelog' label to your PR if this change does not require an entry."
  exit 1
fi
