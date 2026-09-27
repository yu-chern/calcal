import { useEffect, useRef, useState } from 'react'
import type { FormEvent, KeyboardEvent } from 'react'
import { getSession, sendPrompt } from './api'
import type { Session } from './api'
import ConversationHistory from './ConversationHistory'
import { loadConversations, newConversation, STORAGE_KEY } from './conversations'
import type { Conversation } from './conversations'

export default function App() {
  const [chat, setChat] = useState(loadConversations)
  const [session, setSession] = useState<Session | null>(null)
  const [connectionError, setConnectionError] = useState('')
  const [storageError, setStorageError] = useState('')
  const scrollArea = useRef<HTMLDivElement>(null)
  const input = useRef<HTMLTextAreaElement>(null)
  const inFlight = useRef(new Set<string>())
  const active = chat.conversations.find((item) => item.id === chat.activeId)!
  const sending = active.messages.some((message) => message.status === 'sending')
  const limit = session?.max_prompt_chars ?? 4000
  const tooLong = Array.from(active.draft).length > limit

  useEffect(() => {
    let mounted = true
    getSession()
      .then((value) => {
        if (mounted) setSession(value)
      })
      .catch((error: Error) => {
        if (mounted) setConnectionError(error.message)
      })
    return () => {
      mounted = false
    }
  }, [])

  useEffect(() => {
    // Batch draft keystrokes so storage does not block every input event.
    const save = () => {
      try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(chat))
        setStorageError('')
      } catch {
        setStorageError('无法保存历史对话，刷新前请保留所需内容。')
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
    // Reuse the current empty conversation instead of filling history with blank entries.
    if (!active.messages.length && !active.draft) {
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

  async function submit(event?: FormEvent) {
    event?.preventDefault()
    if (!session || !active.draft.trim() || tooLong || inFlight.current.has(active.id)) return
    const conversationId = active.id
    const messageId = crypto.randomUUID()
    const prompt = active.draft
    inFlight.current.add(conversationId)
    updateConversation(conversationId, (item) => ({
      ...item,
      title: item.messages.length
        ? item.title
        : Array.from(prompt.trim().replace(/\s+/g, ' ')).slice(0, 32).join(''),
      updatedAt: Date.now(),
      messages: [...item.messages, { id: messageId, prompt, status: 'sending' }],
    }))
    try {
      const receipt = await sendPrompt(prompt)
      if (typeof receipt.message !== 'string') throw new Error('后端未返回有效回应。')
      updateConversation(conversationId, (item) => ({
        ...item,
        draft: '',
        messages: item.messages.map((message) =>
          message.id === messageId
            ? { ...message, status: 'complete', response: receipt.message }
            : message,
        ),
      }))
    } catch (error) {
      const message = error instanceof Error ? error.message : '发送失败，请重试。'
      updateConversation(conversationId, (item) => ({
        ...item,
        messages: item.messages.map((entry) =>
          entry.id === messageId ? { ...entry, status: 'failed', error: message } : entry,
        ),
      }))
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
          {active.messages.map((message) => (
            <article className="exchange" key={message.id}>
              <div className="user-message">
                <span className="sr-only">你：</span>
                {message.prompt}
              </div>
              <div
                className={`agent-message ${message.status === 'failed' ? 'message-error' : ''}`}
              >
                <span className="sr-only">后端：</span>
                {message.status === 'sending' ? (
                  <span className="typing" role="status" aria-label="等待回应">
                    <i />
                    <i />
                    <i />
                  </span>
                ) : message.status === 'failed' ? (
                  message.error
                ) : (
                  message.response
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
            {connectionError && <button onClick={() => window.location.reload()}>重新连接</button>}
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
            disabled={!session || sending || !active.draft.trim() || tooLong}
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
