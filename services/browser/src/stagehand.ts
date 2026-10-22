import type { Stagehand } from '@browserbasehq/stagehand';
import type { PermissionGate } from '../../../packages/contracts/src/index.js';
/** Stagehand will attach only after a browser action is approved. No agent loop. */
export interface IntelligentBrowser { stagehand: Stagehand | null; act(instruction:string,gate:PermissionGate):Promise<void>; }
export async function isolatedContext():Promise<never>{throw new Error('OUT_OF_SCOPE: isolated sub-agent contexts reserved for a later phase');}
