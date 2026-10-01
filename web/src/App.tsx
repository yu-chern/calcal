import { useEffect, useRef, useState } from 'react'
import type { FormEvent, KeyboardEvent } from 'react'
import { ApiError, getConversation, getRun, getSession, listConversations, sendPrompt } from './api'
import type { Run, Session, Summary } from './api'
import ConversationHistory from './ConversationHistory'
import { fromRun, loadConversations, newConversation, saveDrafts } from './conversations'
import type { ChatState, Conversation } from './conversations'

function mergeSummaries(chat: ChatState, summaries: Summary[]): ChatState {
  const conversations = [...chat.conversations]
  for (const summary of summaries) {
    const existing = conversations.find((c) => c.id === summary.id)
    if (existing) {
      const i = conversations.indexOf(existing)
      conversations[i] = {
        ...existing,
        title: summary.title,
        updatedAt: Date.parse(summary.updated_at),
        persisted: true,
        loaded: existing.persisted ? existing.loaded : false,
      }
    } else
      conversations.push({
        id: summary.id,
        title: summary.title,
        updatedAt: Date.parse(summary.updated_at),
        draft: '',
        messages: [],
        persisted: true,
      })
  }
  return { ...chat, conversations }
}
function applyRun(item: Conversation, run: Run): Conversation {
  const exchange = fromRun(run)
  const terminal = run.status !== 'running'
  const matches = item.pending?.id === run.id
  const existing = item.messages.find((message) => message.id === run.id)
  if (
    item.persisted &&
    !(matches && terminal) &&
    JSON.stringify(existing) === JSON.stringify(exchange)
  )
    return item
  return {
    ...item,
    persisted: true,
    draft: matches && run.status === 'completed' && item.draft === run.prompt ? '' : item.draft,
    pending: matches && terminal ? undefined : item.pending,
    messages: item.messages.some((m) => m.id === run.id)
      ? item.messages.map((m) => (m.id === run.id ? exchange : m))
      : [...item.messages, exchange],
  }
}
export default function App() {
  const [chat, setChat] = useState(loadConversations)
  const [session, setSession] = useState<Session | null>(null)
  const [connectionError, setConnectionError] = useState('')
  const [storageError, setStorageError] = useState('')
  const [historyOffset, setHistoryOffset] = useState(0)
  const [moreHistory, setMoreHistory] = useState(false)
  const [reconnecting, setReconnecting] = useState(false)
  const scrollArea = useRef<HTMLDivElement>(null)
  const input = useRef<HTMLTextAreaElement>(null)
  const inFlight = useRef(new Set<string>())
  const chatRef = useRef(chat)
  useEffect(() => {
    chatRef.current = chat
  }, [chat])
  const active = chat.conversations.find((c) => c.id === chat.activeId)!
  const sending = active.messages.some((m) => m.status === 'sending')
  const limit = session?.max_prompt_chars ?? 4000
  const tooLong = Array.from(active.draft).length > limit

  useEffect(() => {
    let cancelled = false
    Promise.all([getSession(), listConversations()])
      .then(([value, summaries]) => {
        if (cancelled) return
        if (!value.ready) throw new Error('Agent 尚未就绪，请检查服务配置。')
        setSession(value)
        setChat((previous) => mergeSummaries(previous, summaries))
        setHistoryOffset(summaries.length)
        setMoreHistory(summaries.length === 100)
      })
      .catch((error: Error) => {
        if (!cancelled) setConnectionError(error.message)
      })
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    const save = () => {
      try {
        saveDrafts(chat)
        setStorageError('')
      } catch {
        setStorageError('无法保存本机草稿，刷新前请复制尚未发送的内容。')
      }
    }
    const timer = window.setTimeout(save, 150)
    window.addEventListener('pagehide', save)
    return () => {
      window.clearTimeout(timer)
      window.removeEventListener('pagehide', save)
    }
  }, [chat])

  useEffect(() => {
    if (!session || !active.persisted || active.loaded) return
    let cancelled = false
    getConversation(active.id)
      .then((runs) => {
        if (cancelled) return
        setChat((previous) => ({
          ...previous,
          conversations: previous.conversations.map((item) => {
            if (item.id !== active.id) return item
            // Preserve an uncertain submission if the server has not seen it yet.
            const uncertain = item.messages.filter(
              (m) => item.pending?.id === m.id && !runs.some((r) => r.id === m.id),
            )
            let updated: Conversation = {
              ...item,
              loaded: true,
              hasOlder: runs.length === 50,
              messages: [],
            }
            for (const run of runs) updated = applyRun(updated, run)
            return { ...updated, messages: [...updated.messages, ...uncertain] }
          }),
        }))
      })
      .catch((error: Error) => {
        if (!cancelled) setConnectionError(error.message)
      })
    return () => {
      cancelled = true
    }
  }, [session, active.id, active.persisted, active.loaded])

  // Poll all live runs, including conversations hidden in the drawer. No automatic resubmission.
  useEffect(() => {
    if (!session) return
    let cancelled = false
    let timer: number
    const poll = async () => {
      const live = chatRef.current.conversations.flatMap((c) =>
        c.messages
          .filter((m) => m.status === 'sending')
          .map((m) => ({ conversation: c.id, id: m.id })),
      )
      for (const target of live) {
        if (inFlight.current.has(target.conversation)) continue
        try {
          const run = await getRun(target.id)
          if (!cancelled)
            setChat((previous) => ({
              ...previous,
              conversations: previous.conversations.map((c) =>
                c.id === target.conversation ? applyRun(c, run) : c,
              ),
            }))
        } catch (error) {
          if (cancelled) break
          const message =
            error instanceof ApiError && error.status === 404
              ? '尚未确认此消息被接收，草稿已保留。再次发送会使用原请求 ID。'
              : error instanceof Error
                ? error.message
                : '暂时无法读取运行状态。'
          setConnectionError(message)
          setChat((previous) => ({
            ...previous,
            conversations: previous.conversations.map((c) =>
              c.id === target.conversation
                ? {
                    ...c,
                    messages: c.messages.map((m) =>
                      m.id === target.id ? { ...m, status: 'failed', error: message } : m,
                    ),
                  }
                : c,
            ),
          }))
        }
      }
      if (!cancelled) timer = window.setTimeout(() => void poll(), 900)
    }
    void poll()
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [session])

  useEffect(() => {
    const element = scrollArea.current
    if (element) element.scrollTo({ top: element.scrollHeight })
  }, [chat.activeId, active.messages])
  useEffect(() => {
    const element = input.current
    if (element) {
      element.style.height = 'auto'
      element.style.height = `${Math.min(element.scrollHeight, 160)}px`
    }
  }, [active.draft])

  function updateConversation(id: string, update: (item: Conversation) => Conversation) {
    setChat((previous) => ({
      ...previous,
      conversations: previous.conversations.map((item) => (item.id === id ? update(item) : item)),
    }))
  }
  function startConversation() {
    if (!active.messages.length && !active.draft && !active.persisted) {
      input.current?.focus()
      return
    }
    const conversation = newConversation()
    setChat((previous) => ({
      conversations: [conversation, ...previous.conversations],
      activeId: conversation.id,
    }))
    input.current?.focus()
  }
  async function reconnect() {
    setReconnecting(true)
    try {
      const value = await getSession()
      if (!value.ready) throw new Error('Agent 尚未就绪，请检查服务配置。')
      const summaries = await listConversations()
      const recovered: Run[] = []
      for (const item of chatRef.current.conversations) {
        if (!item.pending) continue
        try {
          recovered.push(await getRun(item.pending.id))
        } catch (error) {
          if (!(error instanceof ApiError && error.status === 404)) throw error
        }
      }
      setChat((previous) => {
        const merged = mergeSummaries(previous, summaries)
        return {
          ...merged,
          conversations: merged.conversations.map((c) => {
            let updated: Conversation = { ...c, loaded: !c.persisted }
            for (const run of recovered)
              if (run.conversation_id === c.id) updated = applyRun(updated, run)
            return updated
          }),
        }
      })
      setSession(value)
      setConnectionError('')
    } catch (error) {
      setConnectionError(error instanceof Error ? error.message : '重新连接失败。')
    } finally {
      setReconnecting(false)
    }
  }
  async function loadMoreHistory() {
    try {
      const summaries = await listConversations(historyOffset)
      setChat((previous) => mergeSummaries(previous, summaries))
      setHistoryOffset(historyOffset + summaries.length)
      setMoreHistory(summaries.length === 100)
    } catch (error) {
      setConnectionError((error as Error).message)
    }
  }
  async function loadOlder() {
    try {
      const runs = await getConversation(
        active.id,
        active.messages.filter((message) => message.persisted).length,
      )
      updateConversation(active.id, (item) => ({
        ...item,
        hasOlder: runs.length === 50,
        messages: [
          ...runs.filter((r) => !item.messages.some((m) => m.id === r.id)).map(fromRun),
          ...item.messages,
        ],
      }))
    } catch (error) {
      setConnectionError((error as Error).message)
    }
  }
  async function submit(event?: FormEvent) {
    event?.preventDefault()
    if (
      !session ||
      !active.draft.trim() ||
      tooLong ||
      sending ||
      (active.persisted && !active.loaded) ||
      inFlight.current.has(active.id)
    )
      return
    const conversationId = active.id
    const prompt = active.draft
    const messageId = active.pending?.prompt === prompt ? active.pending.id : crypto.randomUUID()
    inFlight.current.add(conversationId)
    const update = (item: Conversation): Conversation => ({
      ...item,
      title: item.messages.length
        ? item.title
        : Array.from(prompt.trim().replace(/\s+/g, ' ')).slice(0, 32).join(''),
      updatedAt: Date.now(),
      pending: { id: messageId, prompt },
      messages: [
        ...item.messages.filter((m) => m.id !== messageId),
        { id: messageId, prompt, status: 'sending', activity: '正在提交' },
      ],
    })
    // Save the request ID before the network call, so a reload can safely reconcile it.
    try {
      saveDrafts({
        ...chat,
        conversations: chat.conversations.map((c) => (c.id === conversationId ? update(c) : c)),
      })
    } catch {
      setStorageError('无法保存本机草稿，刷新前请复制尚未发送的内容。')
    }
    updateConversation(conversationId, update)
    try {
      await sendPrompt(prompt, conversationId, messageId)
      const run = await getRun(messageId)
      updateConversation(conversationId, (item) => applyRun(item, run))
      setConnectionError('')
    } catch (error) {
      const message = error instanceof Error ? error.message : '发送失败，请重试。'
      updateConversation(conversationId, (item) => ({
        ...item,
        messages: item.messages.map((entry) =>
          entry.id === messageId ? { ...entry, status: 'failed', error: message } : entry,
        ),
      }))
      setConnectionError(message)
    } finally {
      inFlight.current.delete(conversationId)
    }
  }
  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (
      event.key === 'Enter' &&
      !event.shiftKey &&
      !event.nativeEvent.isComposing &&
      event.keyCode !== 229 &&
      window.matchMedia('(pointer: fine)').matches
    ) {
      event.preventDefault()
      void submit()
    }
  }
  return (
    <main className="chat-app">
      <header className="chat-header">
        <ConversationHistory
          conversations={chat.conversations}
          activeId={chat.activeId}
          onSelect={(activeId) => setChat((previous) => ({ ...previous, activeId }))}
          moreHistory={moreHistory}
          onLoadMore={() => void loadMoreHistory()}
        />
        <h1>Calcal</h1>
        <button
          className="icon-button"
          aria-label="新建对话"
          title="新建对话"
          onClick={startConversation}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 5v14M5 12h14" />
          </svg>
        </button>
      </header>
      <div className="conversation-scroll" ref={scrollArea}>
        <div className="messages" role="log" aria-label="对话消息" aria-live="polite">
          {active.persisted && !active.loaded && <p className="activity-line">正在读取对话…</p>}
          {active.hasOlder && (
            <button className="text-button" onClick={() => void loadOlder()}>
              加载更早的消息
            </button>
          )}
          {!active.messages.length && active.loaded && (
            <p className="empty-hint">有什么需要一起算一算？</p>
          )}
          {active.messages.map((message) => (
            <article className="exchange" key={message.id}>
              <div className="user-message">
                <span className="sr-only">你：</span>
                {message.prompt}
              </div>
              <div
                className={`agent-message ${message.status === 'failed' ? 'message-error' : ''}`}
              >
                <span className="sr-only">Calcal：</span>
                {message.reason === 'clarification' &&
                  message.id === active.messages.at(-1)?.id && (
                    <p className="activity-line">等待你补充条件</p>
                  )}
                {message.status === 'sending' ? (
                  <div className="activity-line" role="status">
                    <span className="typing" aria-hidden="true">
                      <i />
                      <i />
                      <i />
                    </span>
                    <span>{message.activity ?? '正在处理'}</span>
                  </div>
                ) : message.status === 'failed' ? (
                  message.error
                ) : (
                  message.response
                )}
                {!!message.activities?.length && (
                  <details className="activity-details">
                    <summary>
                      {message.status === 'sending' ? '查看已执行步骤' : '执行记录'} ·{' '}
                      {message.activities.length} 项
                    </summary>
                    <ol>
                      {message.activities.map((label, index) => (
                        <li key={index}>{label}</li>
                      ))}
                    </ol>
                  </details>
                )}
              </div>
            </article>
          ))}
        </div>
      </div>
      <div className="composer-area">
        {(connectionError || storageError || tooLong) && (
          <div className="composer-error" role="alert">
            {connectionError || storageError || `消息最多 ${limit} 个字符。`}
            {connectionError && (
              <button disabled={reconnecting} onClick={() => void reconnect()}>
                {reconnecting ? '正在连接…' : '重新连接'}
              </button>
            )}
          </div>
        )}
        <form className="composer" onSubmit={submit}>
          <label className="sr-only" htmlFor="prompt">
            输入消息
          </label>
          <textarea
            ref={input}
            id="prompt"
            placeholder="发送消息…"
            rows={1}
            value={active.draft}
            disabled={sending}
            onChange={(event) =>
              updateConversation(active.id, (item) => ({ ...item, draft: event.target.value }))
            }
            onKeyDown={onKeyDown}
          />
          <button
            className="send-button"
            type="submit"
            aria-label="发送消息"
            title="发送消息"
            disabled={
              !session ||
              sending ||
              (active.persisted && !active.loaded) ||
              !active.draft.trim() ||
              tooLong
            }
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M12 19V5m-6 6 6-6 6 6" />
            </svg>
          </button>
        </form>
      </div>
    </main>
  )
}
