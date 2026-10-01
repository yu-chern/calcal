// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import type { Root } from 'react-dom/client'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import App from './App'
import { ApiError, getConversation, getRun, getSession, listConversations, sendPrompt } from './api'
import { STORAGE_KEY } from './conversations'

vi.mock('./api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./api')>()),
  getSession: vi.fn(),
  listConversations: vi.fn(),
  getConversation: vi.fn(),
  getRun: vi.fn(),
  sendPrompt: vi.fn(),
}))
let root: Root
let host: HTMLDivElement
beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  localStorage.clear()
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: true })),
  )
  HTMLElement.prototype.scrollTo = vi.fn()
  vi.mocked(getSession).mockResolvedValue({
    mode: 'agent',
    auth: 'local',
    max_prompt_chars: 4000,
    ready: true,
  })
  vi.mocked(listConversations).mockResolvedValue([])
  vi.mocked(getConversation).mockResolvedValue([])
  host = document.createElement('div')
  document.body.appendChild(host)
  root = createRoot(host)
})
afterEach(async () => {
  await act(async () => root.unmount())
  host.remove()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})
async function mount() {
  await act(async () => root.render(<App />))
}
async function fill(value: string) {
  await act(async () => {
    const input = host.querySelector('textarea')!
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(input, value)
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
}
async function submit() {
  await act(async () => {
    host
      .querySelector('form')!
      .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
  })
}
test('IME confirmation does not send; actual submission exposes activity and disables duplicate sending', async () => {
  vi.mocked(sendPrompt).mockResolvedValue({ run_id: 'run' })
  vi.mocked(getRun).mockImplementation(async (id) => ({
    id,
    conversation_id: 'conversation',
    status: 'running',
    prompt: '计算2+2',
    response: null,
    error: null,
    reason: null,
    activity: '正在计算',
    activities: ['正在分析问题', '正在计算'],
  }))
  await mount()
  await fill('计算2+2')
  await act(async () => {
    host.querySelector('textarea')!.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'Enter',
        isComposing: true,
        keyCode: 229,
        bubbles: true,
      }),
    )
  })
  expect(sendPrompt).not.toHaveBeenCalled()
  await act(async () => {
    host
      .querySelector('textarea')!
      .dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
  })
  expect(sendPrompt).toHaveBeenCalledTimes(1)
  expect(host.textContent).toContain('正在计算')
  expect(host.querySelector('textarea')!.disabled).toBe(true)
  expect(host.querySelector<HTMLButtonElement>('[aria-label="发送消息"]')!.disabled).toBe(true)
  await submit()
  expect(sendPrompt).toHaveBeenCalledTimes(1)
})
test('uncertain failure preserves draft and retries with the same request ID, without duplicating the exchange', async () => {
  vi.mocked(sendPrompt).mockRejectedValueOnce(new ApiError('连接中断，草稿已保留。', 0))
  await mount()
  await fill('失败后重试')
  await submit()
  const [prompt, conversationId, requestId] = vi.mocked(sendPrompt).mock.calls[0]
  expect(host.querySelector('textarea')!.value).toBe(prompt)
  expect(host.textContent).toContain('连接中断')
  const saved = JSON.parse(localStorage.getItem(STORAGE_KEY)!)
  expect(saved.drafts[0].pending.id).toBe(requestId)
  expect(saved.drafts[0].messages).toBeUndefined()
  vi.mocked(sendPrompt).mockResolvedValue({ run_id: requestId })
  vi.mocked(getRun).mockResolvedValue({
    id: requestId,
    conversation_id: conversationId,
    status: 'completed',
    prompt,
    response: '完成',
    error: null,
    reason: 'completed',
    activity: '已完成',
    activities: ['正在分析问题'],
  })
  await submit()
  expect(vi.mocked(sendPrompt).mock.calls[1][2]).toBe(requestId)
  expect(host.querySelectorAll('article')).toHaveLength(1)
  expect(host.querySelector('textarea')!.value).toBe('')
  expect(host.textContent).toContain('完成')
})
test('reload restores a persisted run, polls to completion, and never resubmits the prompt', async () => {
  const conversationId = crypto.randomUUID()
  const id = crypto.randomUUID()
  const prompt = '恢复中的问题'
  localStorage.setItem('calcal.conversations.v1', 'legacy remains untouched')
  localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify({
      activeId: conversationId,
      drafts: [{ id: conversationId, draft: prompt, pending: { id, prompt } }],
    }),
  )
  const run = {
    id,
    conversation_id: conversationId,
    status: 'running' as const,
    prompt,
    response: null,
    error: null,
    reason: null,
    activity: '正在查询日期',
    activities: ['正在查询日期'],
  }
  vi.mocked(listConversations).mockResolvedValue([
    { id: conversationId, title: prompt, updated_at: new Date().toISOString() },
  ])
  vi.mocked(getConversation).mockResolvedValue([run])
  vi.mocked(getRun).mockResolvedValue(run)
  await mount()
  expect(host.textContent).toContain('正在查询日期')
  vi.mocked(getRun).mockResolvedValue({
    ...run,
    status: 'completed',
    response: '日期结果',
    activity: '已完成',
    reason: 'completed',
  })
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000)
  })
  expect(host.textContent).toContain('日期结果')
  expect(host.querySelector('textarea')!.value).toBe('')
  expect(sendPrompt).not.toHaveBeenCalled()
  expect(localStorage.getItem('calcal.conversations.v1')).toBe('legacy remains untouched')
})

test('an unacknowledged draft stays after older database messages when restoring history', async () => {
  const conversationId = crypto.randomUUID()
  const pendingId = crypto.randomUUID()
  localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify({
      activeId: conversationId,
      drafts: [
        {
          id: conversationId,
          draft: '较新的待确认消息',
          pending: { id: pendingId, prompt: '较新的待确认消息' },
        },
      ],
    }),
  )
  vi.mocked(listConversations).mockResolvedValue([
    { id: conversationId, title: '历史', updated_at: new Date().toISOString() },
  ])
  vi.mocked(getConversation).mockResolvedValue([
    {
      id: crypto.randomUUID(),
      conversation_id: conversationId,
      status: 'completed',
      prompt: '较早的消息',
      response: '历史回答',
      error: null,
      reason: 'completed',
      activity: '已完成',
      activities: [],
    },
  ])
  vi.mocked(getRun).mockRejectedValue(new ApiError('not found', 404))
  await mount()
  const exchanges = host.querySelectorAll('article')
  expect(exchanges).toHaveLength(2)
  expect(exchanges[0].textContent).toContain('较早的消息')
  expect(exchanges[1].textContent).toContain('较新的待确认消息')
  expect(sendPrompt).not.toHaveBeenCalled()
})

test('clarification is terminal, visible after reload, and accepts a same-conversation reply', async () => {
  const conversationId = 'f15e9f70-7079-4652-b326-41b1b85c67d2'
  const question = {
    id: 'clarification',
    conversation_id: conversationId,
    status: 'completed' as const,
    reason: 'clarification',
    prompt: '最近一个闰年是哪年？',
    response: '请确认向过去找还是向未来找。',
    error: null,
    activity: '等待澄清',
    activities: ['正在分析问题', '正在确认问题条件', '工具执行完成'],
  }
  localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify({
      activeId: conversationId,
      drafts: [{ id: conversationId, draft: '向过去找' }],
    }),
  )
  vi.mocked(listConversations).mockResolvedValue([
    { id: conversationId, title: question.prompt, updated_at: '2026-10-01T16:00:00Z' },
  ])
  vi.mocked(getConversation).mockResolvedValue([question])
  vi.mocked(sendPrompt).mockResolvedValue({ run_id: 'resumed' })
  vi.mocked(getRun).mockImplementation(async (id) => ({
    ...question,
    id,
    reason: 'completed',
    prompt: '向过去找',
    response: '2024年。',
    activity: '已完成',
  }))
  await mount()
  expect(host.textContent).toContain('等待你补充条件')
  expect(host.textContent).toContain(question.response)
  expect(host.querySelector('textarea')!.disabled).toBe(false)
  expect(host.querySelector('textarea')!.value).toBe('向过去找')
  expect(getRun).not.toHaveBeenCalled()
  await submit()
  expect(sendPrompt).toHaveBeenCalledWith('向过去找', conversationId, expect.any(String))
  expect(host.textContent).toContain('2024年。')
  expect(host.textContent).not.toContain('等待你补充条件')
  expect(host.querySelector('textarea')!.value).toBe('')
})
