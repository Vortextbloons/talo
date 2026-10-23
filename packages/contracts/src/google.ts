
export type GoogleScopeId =
  | 'https://www.googleapis.com/auth/gmail.readonly'
  | 'https://www.googleapis.com/auth/gmail.compose'
  | 'https://www.googleapis.com/auth/calendar.events'
  | 'https://www.googleapis.com/auth/drive.readonly'
  | 'https://www.googleapis.com/auth/drive.file';

export interface GoogleScope {
  id: GoogleScopeId;

  label: string;

  granted: boolean;
}






export interface GoogleStatus {

  configured: boolean;
  connected: boolean;
  account?: string | null;
  scopes: GoogleScope[];
  missingPermissions: string[];
  needsReconnect: boolean;

  authorizing: boolean;
  error?: string | null;
}


export interface GoogleApproval {
  id: string;
  tool: string;
  summary: string;
  arguments: unknown;
  conversationId?: string;
  generationId?: string;
}
