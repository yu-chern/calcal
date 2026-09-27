export type Exchange = {
  id: string
  prompt: string
  status: 'sending' | 'complete' | 'failed'
  response?: string
  error?: string
}

export type Conversation = {
  id: string
  title: string
  draft: string
  updatedAt: number
  messages: Exchange[]
}

export type ChatState = { conversations: Conversation[]; activeId: string }
export const STORAGE_KEY = 'calcal.conversations.v1'

export function newConversation(): Conversation {
  return {
    id: crypto.randomUUID(),
    title: '新对话',
    draft: '',
    updatedAt: Date.now(),
    messages: [],
  }
}

function isConversation(value: unknown): value is Conversation {
  if (!value || typeof value !== 'object') return false
  const item = value as Conversation
  return (
    typeof item.id === 'string' &&
    typeof item.title === 'string' &&
    typeof item.draft === 'string' &&
    typeof item.updatedAt === 'number' &&
    Array.isArray(item.messages) &&
    item.messages.every(
      (message) =>
        message &&
        typeof message.id === 'string' &&
        typeof message.prompt === 'string' &&
        ['sending', 'complete', 'failed'].includes(message.status) &&
        (message.response === undefined || typeof message.response === 'string') &&
        (message.error === undefined || typeof message.error === 'string'),
    )
  )
}

export function loadConversations(): ChatState {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null')
    if (
      saved &&
      Array.isArray(saved.conversations) &&
      saved.conversations.length &&
      saved.conversations.every(isConversation)
    ) {
      const conversations: Conversation[] = saved.conversations.map(
        (conversation: Conversation) => ({
          ...conversation,
          // A refreshed page cannot know whether an in-flight request reached the server.
          messages: conversation.messages.map((message) =>
            message.status === 'sending'
              ? { ...message, status: 'failed', error: '发送已中断，未确认是否收到。' }
              : message,
          ),
        }),
      )
      return {
        conversations,
        activeId: conversations.some((item) => item.id === saved.activeId)
          ? saved.activeId
          : conversations[0].id,
      }
    }
  } catch {
    /* Storage may be disabled or contain data from an older version. */
  }
  const conversation = newConversation()
  return { conversations: [conversation], activeId: conversation.id }
}
