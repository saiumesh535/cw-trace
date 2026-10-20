/** Turn raw AWS SDK / Tauri error strings into short, actionable UI copy. */

export type FriendlyAwsError = {
  kind: "auth" | "error";
  title: string;
  /** One-line suggestion of what to do next. */
  hint: string;
  /** Optional shell command the user can copy/run. */
  command?: string;
  /** Truncated raw text for “Show details”. */
  raw: string;
};

export type AwsErrorContext = {
  profile?: string;
  region?: string;
};

function oneLine(s: string, max = 280): string {
  const t = s.replace(/\s+/g, " ").trim();
  return t.length <= max ? t : `${t.slice(0, max - 1)}…`;
}

function loginCommand(profile?: string): string {
  const p = profile?.trim();
  if (p) return `aws sso login --profile ${p}`;
  return "aws sso login";
}

export function friendlyAwsError(rawInput: unknown, ctx: AwsErrorContext = {}): FriendlyAwsError {
  const raw = oneLine(String(rawInput ?? "Unknown error"), 800);
  const lower = raw.toLowerCase();
  const profile = ctx.profile?.trim() || undefined;
  const region = ctx.region?.trim() || undefined;
  const where = [profile ? `profile “${profile}”` : null, region ? `region ${region}` : null]
    .filter(Boolean)
    .join(", ");

  // Missing / unloaded credentials (the dump in the screenshot).
  if (
    /credentialsnotloaded|credentials not loaded|providerchainerror|environment variable not set|no credentials|could not load credentials|failed to load credentials/i.test(
      raw,
    )
  ) {
    return {
      kind: "auth",
      title: "AWS credentials not found",
      hint: profile
        ? `Sign in for ${where}, then hit Retry.`
        : "This stage has no AWS profile set. Edit the stage and pick a profile, or sign in to the default chain.",
      command: loginCommand(profile),
      raw,
    };
  }

  // Expired SSO / session token.
  if (/expired|sso session|token.*expired|refresh.*token|InvalidClientTokenId|ExpiredToken/i.test(raw)) {
    return {
      kind: "auth",
      title: "AWS session expired",
      hint: profile
        ? `Your SSO session for “${profile}” expired. Sign in again, then Retry.`
        : "Your AWS session expired. Sign in again, then Retry.",
      command: loginCommand(profile),
      raw,
    };
  }

  // Profile missing from ~/.aws/config.
  if (/profile not found|could not find profile|unknown profile|NoSuchProfile/i.test(raw)) {
    return {
      kind: "auth",
      title: profile ? `Profile “${profile}” not found` : "AWS profile not found",
      hint: "Check the name in ~/.aws/config, or edit this stage and pick a profile that exists.",
      raw,
    };
  }

  // Permission denied.
  if (/accessdenied|not authorized|unauthorized|is not authorized to perform/i.test(raw)) {
    return {
      kind: "error",
      title: "Permission denied",
      hint: where
        ? `This identity can’t read CloudWatch/X-Ray for ${where}. Ask for logs:FilterLogEvents / xray:BatchGetTraces.`
        : "This identity can’t read CloudWatch or X-Ray. Check the IAM policy.",
      raw,
    };
  }

  // Wrong region / missing log group (already somewhat friendly from Rust).
  if (/log group does not exist|ResourceNotFoundException/i.test(raw)) {
    return {
      kind: "error",
      title: "Log group not in this region",
      hint: region
        ? `Nothing matched in ${region}. Deploy the stack, or edit the stage region/profile.`
        : "Deploy the stack, or check the stage’s region and profile.",
      raw,
    };
  }

  if (/trace .* not (found|yet available)|unprocessed|TraceNotFound/i.test(raw)) {
    return {
      kind: "error",
      title: "Trace not available",
      hint: "X-Ray may still be processing it, or it expired / isn’t in this region. Wait a few seconds and try again.",
      raw,
    };
  }

  if (/throttl|TooManyRequests|Rate exceeded/i.test(raw)) {
    return {
      kind: "error",
      title: "AWS is throttling requests",
      hint: "Wait a moment, then Retry. Narrow the time range if this keeps happening.",
      raw,
    };
  }

  if (/timeout|timed out|connection error|network/i.test(raw)) {
    return {
      kind: "error",
      title: "Couldn’t reach AWS",
      hint: "Check your network / VPN, then Retry.",
      raw,
    };
  }

  // Opaque SDK “dispatch failure” with Credentials buried in Debug.
  if (/dispatch failure/i.test(raw) && /credential/i.test(raw)) {
    return {
      kind: "auth",
      title: "AWS credentials not found",
      hint: profile
        ? `Sign in for profile “${profile}”, then hit Retry.`
        : "Sign in to AWS (or set a profile on this stage), then Retry.",
      command: loginCommand(profile),
      raw,
    };
  }

  if (/dispatch failure|service error/i.test(lower)) {
    return {
      kind: "error",
      title: "AWS request failed",
      hint: "Retry in a moment. If it keeps failing, check profile, region, and network.",
      command: profile ? loginCommand(profile) : undefined,
      raw,
    };
  }

  return {
    kind: "error",
    title: "Something went wrong talking to AWS",
    hint: "Retry, or edit the stage’s profile and region.",
    raw,
  };
}
