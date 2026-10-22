export const PRIMARY_CONVERSATION_ID = 'talo-primary';

export interface TelegramStatus {
  connected: boolean;
  botUsername?: string;
  paired: boolean;
  accountName?: string;
  pairingUrl?: string;
  pendingAccount?: { name: string };
  error?: string;
  online: boolean;
}
