import type { TFunction } from "i18next";

function truncateFirstLine(message: string, maxLen: number): string {
  const line = message.split("\n")[0]?.trim() ?? message;
  if (line.length <= maxLen) return line;
  return `${line.slice(0, maxLen - 1)}…`;
}

/**
 * Short, user-facing text for task `error` strings. Long anyhow chains are summarized;
 * use the original string as a tooltip `title` where needed.
 */
export function getFriendlyTaskError(message: string, t: TFunction): string {
  const normalized = message.toLowerCase();

  if (
    normalized.includes("operation timed out") ||
    normalized.includes("os error 60") ||
    (normalized.includes("timed out") &&
      (normalized.includes("tcp") ||
        normalized.includes("connect") ||
        normalized.includes("error sending request")))
  ) {
    return t(
      "popup.taskErrorNetworkTimeout",
      "Connection timed out. Check your network and try again."
    );
  }

  if (
    normalized.includes("failed to send download request") ||
    normalized.includes("error sending request for url") ||
    normalized.includes("client error (connect)") ||
    normalized.includes("tcp connect error") ||
    normalized.includes("connection refused") ||
    normalized.includes("network is unreachable") ||
    normalized.includes("no route to host")
  ) {
    return t(
      "popup.taskErrorNetworkGeneric",
      "Network error while downloading. Check your connection and try again."
    );
  }

  if (
    normalized.includes("modification time does not match") ||
    normalized.includes("metadata mismatch")
  ) {
    return t(
      "popup.objectExistedMismatchTaskError",
      "The server has a file with the same name but different size or date. Resolve the conflict or overwrite from the client/web UI."
    );
  }
  if (normalized.includes("40004") || normalized.includes("object existed")) {
    return t(
      "popup.objectExistedTaskError",
      "A file with this name already exists on the server. If sync keeps failing, remove the local copy or resolve duplicates in the web UI."
    );
  }
  if (normalized.includes("40076") || normalized.includes("stale version")) {
    return t(
      "popup.staleVersionTaskError",
      "The server has a newer version of this file. Open the conflict workflow or choose overwrite in settings if available."
    );
  }
  if (normalized.includes("conflict") || normalized.includes("409")) {
    return t(
      "popup.conflictTaskError",
      "This file conflicted with a remote change. Review the conflict before continuing."
    );
  }
  if (
    (normalized.includes("credential") && normalized.includes("expired")) ||
    normalized.includes("unauthorized") ||
    normalized.includes("401")
  ) {
    return t(
      "popup.credentialExpiredTaskError",
      "Drive credentials expired. Reauthorize this drive to continue syncing."
    );
  }

  return truncateFirstLine(message, 160);
}
