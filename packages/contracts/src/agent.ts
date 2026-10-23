


export type Capability = 'web' | 'browser' | 'files' | 'google';
export type MemoryScope = 'inherit' | 'workspace' | 'off';






export interface WorkspaceBundle {
  id: string; name: string; folder: string | null; archived: boolean;
  instructions: string; pinned: string[]; memoryScope: MemoryScope;
  capabilities: Capability[]; agentOwned: boolean; draft: boolean; createdAt: string;
}


export interface AskOption { id: string; label: string; detail: string }
export interface AskQuestion {
  id: string; question: string; options: AskOption[];

  recommended: string; other: boolean;
}

export interface PendingAsk {
  id: string; conversationId: string; header: string; questions: AskQuestion[];
  createdAt: number; expiresAt: number;

  parentConversationId?: string; taskId?: string; taskTitle?: string;
}

export interface AskAnswer { questionId: string; optionId?: string; label?: string; other?: string; decided?: boolean }

export type SkillStatus = 'proposed' | 'approved' | 'disabled';





export interface Skill {
  id: string;

  name: string;

  title: string;
  description: string; instructions: string;

  workspaceId: string | null;
  status: SkillStatus; revision: number;

  replaces: string | null;
  sourceConversationId: string | null;
  createdAt: string; updatedAt: string;
}

export interface AgentBudget { used: number; limit: number }
export interface AgentState {
  asks: PendingAsk[];
  drafts: WorkspaceBundle[]; skills: Skill[]; skillProposals: Skill[];
  budget: AgentBudget;
}
