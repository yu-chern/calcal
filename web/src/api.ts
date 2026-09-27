export type Session = { mode: 'print'; auth: 'local' | 'cloudflare'; max_prompt_chars: number }
export type Receipt = { status: 'printed'; message: string }

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response
  try {
    response = await fetch(path, {
      ...init,
      credentials: 'same-origin',
      signal: AbortSignal.timeout(15_000),
    })
  } catch {
    throw new Error('连接中断或超时，无法确认是否已打印。请检查后端，再决定是否重发。')
  }
  if (
    response.redirected ||
    response.status === 401 ||
    !response.headers.get('content-type')?.includes('application/json')
  ) {
    throw new Error('会话失效或服务暂不可用，请刷新页面重新连接。')
  }
  const body = await response.json()
  if (!response.ok) throw new Error(body.error ?? '发送失败，请稍后重试。')
  return body as T
}

export const getSession = () => request<Session>('/api/session')
export const sendPrompt = (prompt: string) =>
  request<Receipt>('/api/messages', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ prompt }),
  })
