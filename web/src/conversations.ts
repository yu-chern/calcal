import type { Run } from './api'
export type Exchange = {
  persisted?: boolean
  id: string
  prompt: string
  status: 'sending' | 'complete' | 'failed'
  response?: string
  reason?: string
  error?: string
  activity?: string
  activities?: string[]
}
export type Conversation = {
  id: string
  title: string
  draft: string
  updatedAt: number
  messages: Exchange[]
  pending?: { id: string; prompt: string }
  persisted?: boolean
  loaded?: boolean
  hasOlder?: boolean
}
export type ChatState = { conversations: Conversation[]; activeId: string }
export const STORAGE_KEY = 'calcal.drafts.v2'
export function newConversation(): Conversation {
  return {
    id: crypto.randomUUID(),
    title: '新对话',
    draft: '',
    updatedAt: Date.now(),
    messages: [],
    loaded: true,
  }
}
export function fromRun(run: Run): Exchange {
  return {
    persisted: true,
    id: run.id,
    prompt: run.prompt,
    status:
      run.status === 'running' ? 'sending' : run.status === 'completed' ? 'complete' : 'failed',
    response: run.response ?? undefined,
    reason: run.reason ?? undefined,
    error: run.error ?? undefined,
    activity: run.activity,
    activities: run.activities,
  }
}
// Only drafts and uncertain request IDs are local. Postgres owns conversation history.
export function saveDrafts(chat: ChatState) {
  localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify({
      activeId: chat.activeId,
      drafts: chat.conversations
        .filter((c) => c.draft || c.pending || !c.persisted || c.id === chat.activeId)
        .map((c) => ({ id: c.id, draft: c.draft, pending: c.pending })),
    }),
  )
}
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
export function loadConversations(): ChatState {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null')
    if (Array.isArray(saved?.drafts)) {
      const conversations: Conversation[] = []
      for (const item of saved.drafts) {
        if (
          !item ||
          typeof item.id !== 'string' ||
          !uuid.test(item.id) ||
          typeof item.draft !== 'string'
        )
          continue
        if (conversations.some((c) => c.id === item.id)) continue
        const pending =
          item.pending && uuid.test(item.pending.id) && typeof item.pending.prompt === 'string'
            ? item.pending
            : undefined
        conversations.push({
          ...newConversation(),
          id: item.id,
          draft: item.draft,
          pending,
          messages: pending
            ? [
                {
                  id: pending.id,
                  prompt: pending.prompt,
                  status: 'sending',
                  activity: '正在恢复运行状态',
                },
              ]
            : [],
        })
      }
      if (conversations.length)
        return {
          conversations,
          activeId: conversations.some((c) => c.id === saved.activeId)
            ? saved.activeId
            : conversations[0].id,
        }
    }
  } catch {
    /* Storage may be disabled. The UI reports failed writes. */
  }
  const conversation = newConversation()
  return { conversations: [conversation], activeId: conversation.id }
}
