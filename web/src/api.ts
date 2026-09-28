export type Session = {
  mode: 'agent'
  auth: 'local' | 'cloudflare'
  max_prompt_chars: number
  ready: boolean
}
export type Summary = { id: string; title: string; updated_at: string }
export type Run = {
  id: string
  conversation_id: string
  status: 'running' | 'completed' | 'failed'
  prompt: string
  response: string | null
  error: string | null
  reason: string | null
  activity: string
  activities: string[]
}
export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message)
  }
}
async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response
  try {
    response = await fetch(path, {
      ...init,
      credentials: 'same-origin',
      signal: AbortSignal.timeout(15_000),
    })
  } catch {
    throw new ApiError('连接中断，任务可能仍在运行。请重新连接查看进度；草稿已保留。', 0)
  }
  if (
    response.redirected ||
    response.status === 401 ||
    !response.headers.get('content-type')?.includes('application/json')
  ) {
    throw new ApiError('会话失效或服务暂不可用，请刷新页面重新连接。', response.status)
  }
  const body = await response.json()
  if (!response.ok) throw new ApiError(body.error ?? '请求失败，请稍后重试。', response.status)
  return body as T
}
export const getSession = () => request<Session>('/api/session')
export const listConversations = (offset = 0) =>
  request<Summary[]>(`/api/conversations?offset=${offset}`)
export const getConversation = (id: string, offset = 0) =>
  request<Run[]>(`/api/conversations/${id}?offset=${offset}`)
export const getRun = (id: string) => request<Run>(`/api/runs/${id}`)
export const sendPrompt = (prompt: string, conversation_id: string, request_id: string) =>
  request<{ run_id: string }>('/api/messages', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ prompt, conversation_id, request_id }),
  })
