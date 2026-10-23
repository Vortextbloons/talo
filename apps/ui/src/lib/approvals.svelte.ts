import type { GoogleApproval } from '../../../../packages/contracts/src/google';

export const approvals = $state({ google: [] as GoogleApproval[] });

export function setGoogleApprovals(value: GoogleApproval[]): void {
  approvals.google = value;
}
