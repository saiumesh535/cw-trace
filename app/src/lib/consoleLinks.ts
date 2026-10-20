/** Read-only AWS console deep links for the active stage context. */

export function cloudWatchLogGroupUrl(region: string, logGroup: string): string {
  const r = region || "us-east-1";
  const enc = encodeURIComponent(logGroup);
  return `https://${r}.console.aws.amazon.com/cloudwatch/home?region=${r}#logsV2:log-groups/log-group/${enc}`;
}

export function xrayTraceUrl(region: string, traceId: string): string {
  const r = region || "us-east-1";
  return `https://${r}.console.aws.amazon.com/xray/home?region=${r}#/traces/${encodeURIComponent(traceId)}`;
}

export function xrayTracesUrl(region: string): string {
  const r = region || "us-east-1";
  return `https://${r}.console.aws.amazon.com/xray/home?region=${r}#/traces`;
}
