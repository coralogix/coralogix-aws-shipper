#!/usr/bin/env bash
# Smoke-tests a published GovCloud shipper version: deploys template-govcloud.yaml with the
# CloudWatch integration from the published artifacts, writes a unique marker into the
# source log group, and passes only when the marker reaches the Coralogix test team.
#
# Usage: smoke.sh <version> <region> <arm64|x86_64>
#
# Environment:
#   SMOKE_DOMAIN           Coralogix domain of the test team, e.g. eu2.coralogix.com (required)
#   SMOKE_QUERY_KEY        Coralogix API key with DataQuerying permissions for that team, or
#   SMOKE_QUERY_CMD        a command run as `$SMOKE_QUERY_CMD "<dataprime query>"` that prints
#                          the matching logs (for local runs, e.g. with the cx CLI)
#   SMOKE_SECRET_NAME      Secrets Manager secret with the test team's Send-Your-Data key
#                          (default coralogix-gov-smoke-test-api-key)
#   SMOKE_ARTIFACT_BUCKET  bucket with the published artifacts
#                          (default coralogix-public-artifacts-<region>)
#   SMOKE_BOUNDARY_ARN     permissions boundary for the roles the stack creates (optional)
#   SMOKE_TEMPLATE         template to deploy (default template-govcloud.yaml)
#   SMOKE_RUN_ID           unique id for resource names (default GITHUB_RUN_ID or the time)
#
# Every resource is named smoke-* and removed at the end, pass or fail. The exit code is
# the test's; cleanup problems are reported but never hide a test failure.
set -uo pipefail

version=${1:?usage: smoke.sh <version> <region> <arm64|x86_64>}
region=${2:?usage: smoke.sh <version> <region> <arm64|x86_64>}
arch=${3:?usage: smoke.sh <version> <region> <arm64|x86_64>}

case "$arch" in
  arm64) key_arch=arm64 ;;
  x86_64) key_arch=x86-64 ;;
  *) echo "::error::arch must be arm64 or x86_64, not '$arch'."; exit 2 ;;
esac
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "::error::'$version' is not a version like 1.5.3."; exit 2; }
: "${SMOKE_DOMAIN:?SMOKE_DOMAIN is required}"
if [ -z "${SMOKE_QUERY_KEY:-}" ] && [ -z "${SMOKE_QUERY_CMD:-}" ]; then
  echo "::error::Set SMOKE_QUERY_KEY or SMOKE_QUERY_CMD to read the marker back from Coralogix."
  exit 2
fi

export AWS_REGION="$region" AWS_DEFAULT_REGION="$region"
secret_name=${SMOKE_SECRET_NAME:-coralogix-gov-smoke-test-api-key}
artifact_bucket=${SMOKE_ARTIFACT_BUCKET:-coralogix-public-artifacts-$region}
template=${SMOKE_TEMPLATE:-template-govcloud.yaml}
run_id=${SMOKE_RUN_ID:-${GITHUB_RUN_ID:-$(date +%s)}}

name="smoke-${run_id}-${key_arch}"
source_group="/aws/lambda/${name}-src"
subsystem="$name"
marker="cxsmoke ${version} ${arch} ${run_id} $(od -An -N6 -tx1 /dev/urandom | tr -d ' \n')"
account=$(aws sts get-caller-identity --query Account --output text) || exit 1
template_bucket="${name}-${account}"
now_ms() { python3 -c 'import time; print(int(time.time() * 1000))'; }
log() { echo "[$(date -u +%H:%M:%S)] $*"; }

result=1
shipper_group=""
created_stack=false created_bucket=false

# Deletes the stack, optionally keeping the given logical resources, and waits up to 10
# minutes. Prints the final status; only a stack that no longer exists counts as deleted.
delete_stack() {
  local retain=() status="" out
  [ "$#" -gt 0 ] && retain=(--retain-resources "$@")
  if ! out=$(aws cloudformation delete-stack --stack-name "$name" ${retain[@]+"${retain[@]}"} 2>&1); then
    echo "delete-stack failed: $(tail -n1 <<<"$out")"; return
  fi
  for _ in $(seq 1 60); do
    if out=$(aws cloudformation describe-stacks --stack-name "$name" \
        --query 'Stacks[0].StackStatus' --output text 2>&1); then
      status=$out
      case "$status" in DELETE_COMPLETE|DELETE_FAILED) break ;; esac
    elif grep -q 'does not exist' <<<"$out"; then
      echo DELETE_COMPLETE; return
    else
      # Denied, expired credentials or a network error: unknown, not deleted. Keep polling.
      status="unknown: $(tail -n1 <<<"$out")"
    fi
    sleep 10
  done
  echo "$status"
}

cleanup() {
  local leftovers=() failed_ids=() status fn failed out
  log "cleanup"
  if [ "$created_stack" = true ]; then
    # The shipper's function name is only known from the stack, so read it before
    # deleting, also when the stack never completed.
    if [ -z "$shipper_group" ]; then
      fn=$(aws cloudformation describe-stack-resources --stack-name "$name" --logical-resource-id LambdaFunction \
        --query 'StackResources[0].PhysicalResourceId' --output text 2>/dev/null) || fn=""
      [ -n "$fn" ] && [ "$fn" != None ] && shipper_group="/aws/lambda/$fn"
    fi
    status=$(delete_stack)
    if [ "$status" = DELETE_FAILED ]; then
      # Usually a custom resource that could not undo its work. Retry, keeping the
      # resources that failed, so the rest of the stack still goes.
      failed=$(aws cloudformation describe-stack-resources --stack-name "$name" \
        --query "StackResources[?ResourceStatus=='DELETE_FAILED'].LogicalResourceId" --output text 2>/dev/null)
      read -ra failed_ids <<<"$failed"
      status=$(delete_stack ${failed_ids[@]+"${failed_ids[@]}"})
      [ -z "$failed" ] || leftovers+=("resources kept by stack deletion: $failed")
    fi
    [ "$status" = DELETE_COMPLETE ] || leftovers+=("stack $name ($status)")
  fi
  # The shipper's log group is retained by the template; the helper Lambdas create theirs.
  # Deleted without checking first: listing log groups needs wider permissions.
  for group in "$source_group" "$shipper_group" "/aws/lambda/${name}-custom-resource" "/aws/lambda/${name}-iam-waiter"; do
    [ -n "$group" ] || continue
    if ! out=$(aws logs delete-log-group --log-group-name "$group" 2>&1); then
      grep -q ResourceNotFoundException <<<"$out" || leftovers+=("log group $group")
    fi
  done
  if [ "$created_bucket" = true ]; then
    { aws s3 rm "s3://$template_bucket" --recursive --quiet && aws s3api delete-bucket --bucket "$template_bucket"; } \
      || leftovers+=("bucket $template_bucket")
  fi
  if [ "${#leftovers[@]}" -gt 0 ]; then
    for item in "${leftovers[@]}"; do echo "::warning::smoke cleanup left: $item"; done
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
      printf -- '- cleanup left: %s\n' "${leftovers[@]}" >> "$GITHUB_STEP_SUMMARY"
    fi
  fi
  log "result: $([ "$result" -eq 0 ] && echo PASS || echo FAIL)"
  exit "$result"
}
# Installed before the first resource exists, so every exit path cleans up.
trap cleanup EXIT
trap 'exit 1' INT TERM

fail() { echo "::error::$*"; result=1; exit 1; }

secret_arn=$(aws secretsmanager describe-secret --secret-id "$secret_name" --query ARN --output text) \
  || fail "Can't find the smoke API key secret '$secret_name'."

log "template bucket $template_bucket"
created_bucket=true
if [ "$region" = us-east-1 ]; then
  aws s3api create-bucket --bucket "$template_bucket" >/dev/null || fail "Can't create $template_bucket."
else
  aws s3api create-bucket --bucket "$template_bucket" \
    --create-bucket-configuration "LocationConstraint=$region" >/dev/null || fail "Can't create $template_bucket."
fi
# The template is larger than CloudFormation's 51,200-byte inline limit.
aws s3api put-object --bucket "$template_bucket" --key template.yaml --body "$template" >/dev/null \
  || fail "Can't upload the template."
template_url="https://${template_bucket}.s3.${region}.amazonaws.com/template.yaml"

log "source log group $source_group"
aws logs create-log-group --log-group-name "$source_group" || fail "Can't create $source_group."

params=(
  "ParameterKey=IntegrationType,ParameterValue=CloudWatch"
  "ParameterKey=CloudWatchLogGroupName,ParameterValue=$source_group"
  "ParameterKey=CustomDomain,ParameterValue=$SMOKE_DOMAIN"
  "ParameterKey=ApiKey,ParameterValue=$secret_arn"
  "ParameterKey=StoreAPIKeyInSecretsManager,ParameterValue=false"
  "ParameterKey=ApplicationName,ParameterValue=smoke"
  "ParameterKey=SubsystemName,ParameterValue=$subsystem"
  # The FIPS startup line is logged at INFO; the template defaults to WARN.
  "ParameterKey=LogLevel,ParameterValue=INFO"
  "ParameterKey=FunctionArchitectures,ParameterValue=$arch"
  "ParameterKey=LambdaCodeBucket,ParameterValue=$artifact_bucket"
  "ParameterKey=LambdaCodeKey,ParameterValue=coralogix-aws-shipper-${key_arch}-${version}.zip"
  "ParameterKey=CustomResourceCodeBucket,ParameterValue=$artifact_bucket"
  "ParameterKey=CustomResourceCodeKey,ParameterValue=coralogix-aws-shipper-custom-resource-govcloud-${version}.zip"
)
[ -n "${SMOKE_BOUNDARY_ARN:-}" ] && params+=("ParameterKey=PermissionsBoundaryArn,ParameterValue=$SMOKE_BOUNDARY_ARN")

log "stack $name: shipper $version $arch from s3://$artifact_bucket"
created_stack=true
stack_start_ms=$(now_ms)
aws cloudformation create-stack --stack-name "$name" --template-url "$template_url" \
  --capabilities CAPABILITY_IAM CAPABILITY_NAMED_IAM --parameters "${params[@]}" >/dev/null \
  || fail "create-stack was refused."

status=""
for _ in $(seq 1 60); do
  status=$(aws cloudformation describe-stacks --stack-name "$name" --query 'Stacks[0].StackStatus' --output text) || true
  case "$status" in
    CREATE_COMPLETE) break ;;
    *ROLLBACK*|*FAILED*)
      aws cloudformation describe-stack-events --stack-name "$name" \
        --query "StackEvents[?contains(ResourceStatus,'FAILED')].[LogicalResourceId,ResourceStatusReason]" --output text | head -10
      fail "Stack $name ended in $status." ;;
  esac
  sleep 10
done
[ "$status" = CREATE_COMPLETE ] || fail "Stack $name not complete after 10 minutes ($status)."

function_arn=$(aws cloudformation describe-stacks --stack-name "$name" \
  --query "Stacks[0].Outputs[?OutputKey=='LambdaArn'].OutputValue" --output text)
[ -n "$function_arn" ] && [ "$function_arn" != None ] || fail "Stack has no LambdaArn output."
shipper_group="/aws/lambda/${function_arn##*:}"

log "marker: $marker"
aws logs create-log-stream --log-group-name "$source_group" --log-stream-name smoke || fail "Can't create the log stream."
marker_ms=$(now_ms)
aws logs put-log-events --log-group-name "$source_group" --log-stream-name smoke \
  --log-events "timestamp=$marker_ms,message=$marker" >/dev/null || fail "Can't write the marker."

count() { # <start ms> <filter pattern>; fails, printing nothing, when the logs can't be read
  # JSON output merges every page; --query with text output prints one count per page.
  local out
  out=$(aws logs filter-log-events --log-group-name "$shipper_group" --start-time "$1" \
    --filter-pattern "$2" --output json 2>/dev/null) && jq -e '.events | length' <<<"$out" 2>/dev/null
}

query_marker() {
  local dataprime="source logs | filter \$l.subsystemname == '$subsystem' | limit 20"
  if [ -n "${SMOKE_QUERY_CMD:-}" ]; then
    $SMOKE_QUERY_CMD "$dataprime" 2>/dev/null
  else
    local start end
    start=$(python3 -c "import datetime as d; print(d.datetime.fromtimestamp($marker_ms/1000 - 300, d.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))")
    end=$(python3 -c "import datetime as d; print((d.datetime.now(d.timezone.utc) + d.timedelta(minutes=5)).strftime('%Y-%m-%dT%H:%M:%SZ'))")
    jq -n --arg q "$dataprime" --arg s "$start" --arg e "$end" \
      '{query: $q, metadata: {tier: "TIER_FREQUENT_SEARCH", syntax: "QUERY_SYNTAX_DATAPRIME", startDate: $s, endDate: $e, limit: 20}}' \
      | curl -fsS --connect-timeout 10 --max-time 20 -X POST "https://api.${SMOKE_DOMAIN}/api/v1/dataprime/query" \
          -H 'Content-Type: application/json' -H "Authorization: Bearer $SMOKE_QUERY_KEY" --data @- 2>/dev/null
  fi
}

fips=0 invoked=0 errors=0 errors_checked=false delivered=false
# A wall-clock deadline, not a count, so slow queries can't stretch it. Worst case for the
# whole script: creation 10 + verification 10 + deletion 2 x 10 minutes, inside the job's 60.
deadline=$(( $(date +%s) + 600 ))
while [ "$(date +%s)" -lt "$deadline" ]; do
  sleep 15
  # FIPS mode is logged at cold start, which can come before the marker (for example on
  # the subscription's control message), so it is searched from stack creation.
  # A failed read counts as not found yet for FIPS and invocations, so it can only delay
  # a pass. For errors it would hide a failure, so the pass needs a successful check.
  [ "$fips" -gt 0 ] || fips=$(count "$stack_start_ms" '"AWS FIPS mode enabled for SDK HTTP client"') || fips=0
  invoked=$(count "$marker_ms" REPORT) || invoked=0
  if errors=$(count "$marker_ms" '?ERROR ?Error ?"Status: error" ?"Failed to send logs"'); then
    errors_checked=true
  else
    errors=0 errors_checked=false
  fi
  [ "$errors" -eq 0 ] || break
  if ! $delivered && query_marker | grep -qF "$marker"; then delivered=true; fi
  log "fips=$fips invocations-after-marker=$invoked errors=$errors (checked: $errors_checked) delivered=$delivered"
  if [ "$fips" -gt 0 ] && [ "$invoked" -gt 0 ] && $delivered && $errors_checked; then break; fi
done

if [ "$errors" -gt 0 ]; then
  aws logs filter-log-events --log-group-name "$shipper_group" --start-time "$marker_ms" \
    --filter-pattern '?ERROR ?Error ?"Status: error" ?"Failed to send logs"' --query 'events[].message' --output text | head -20
  fail "The shipper logged errors after the marker."
fi
$errors_checked || fail "Couldn't read $shipper_group to check for errors."
[ "$fips" -gt 0 ] || fail "No 'AWS FIPS mode enabled' line in $shipper_group."
[ "$invoked" -gt 0 ] || fail "The shipper did not run after the marker was written."
$delivered || fail "The marker did not reach Coralogix ($SMOKE_DOMAIN, subsystem $subsystem) within 10 minutes."

log "PASS: $version $arch deployed, ran in FIPS mode and delivered the marker"
[ -n "${GITHUB_STEP_SUMMARY:-}" ] && echo "- $version $arch: PASS" >> "$GITHUB_STEP_SUMMARY"
result=0
