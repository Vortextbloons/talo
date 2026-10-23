/** Host-owned Google Workspace integration contract. Tokens never cross this boundary. */
export type GoogleScopeId =
  | 'https://www.googleapis.com/auth/gmail.readonly'
  | 'https://www.googleapis.com/auth/gmail.compose'
  | 'https://www.googleapis.com/auth/calendar.events'
  | 'https://www.googleapis.com/auth/drive.readonly'
  | 'https://www.googleapis.com/auth/drive.file';

export interface GoogleScope {
  id: GoogleScopeId;
  /** Plain-language description shown beside the permission. */
  label: string;
  /** Google's granular consent may withhold an individual scope. */
  granted: boolean;
}

/**
 * Tauri `google_action({ operation, payload? })` operations:
 * state | approvals | connect | cancel | disconnect | resolve.
 * `resolve` takes `{id, decision:'once'|'deny'}`.
 */
export interface GoogleStatus {
  /** The app's Google OAuth client is built in, so Connect Google can start. */
  configured: boolean;
  connected: boolean;
  account?: string | null;
  scopes: GoogleScope[];
  missingPermissions: string[];
  needsReconnect: boolean;
  /** A sign-in is waiting for the browser's loopback callback. */
  authorizing: boolean;
  error?: string | null;
}

/** A write that is waiting for an exact-operation approval in this chat. */
export interface GoogleApproval {
  id: string;
  tool: string;
  summary: string;
  arguments: unknown;
}
