# Least-privilege IAM policy for cw-trace

cw-trace is **read-only against AWS**. It never writes to CloudWatch or
X-Ray. The policy below is sufficient for every feature (construct tree,
log search, pagination, Insights, StartLiveTail, X-Ray waterfalls).

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "LogsRead",
      "Effect": "Allow",
      "Action": [
        "logs:DescribeLogGroups",
        "logs:FilterLogEvents",
        "logs:StartLiveTail",
        "logs:StartQuery",
        "logs:GetQueryResults"
      ],
      "Resource": "*"
    },
    {
      "Sid": "XRayRead",
      "Effect": "Allow",
      "Action": [
        "xray:GetTraceSummaries",
        "xray:BatchGetTraces"
      ],
      "Resource": "*"
    }
  ]
}
```

Notes:

- `logs:DescribeLogGroups` and `xray:GetTraceSummaries` do not support
  resource-level scoping; `"Resource": "*"` is required for them. To narrow
  data access, use a region or account boundary via the stage's
  profile/region (each AWS call carries the stage's profile + region; the
  app also supports a per-session region override for multi-region stacks).
- `logs:StartLiveTail` is only needed for the Tail button / `--live`. The app
  falls back to polling `FilterLogEvents` when the API is unavailable.
- `logs:StartQuery` + `GetQueryResults` are only needed for the Insights
  panel / `cw-trace insights`. Queries cost per GB scanned.
- No KMS permissions are required for reading logs whose data key the
  caller can already decrypt via CloudWatch's service role.

The app authenticates exclusively through the standard AWS credential chain
(profiles, SSO, env vars). No credentials are stored by cw-trace.
