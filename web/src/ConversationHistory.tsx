import { useRef } from 'react'
import type { Conversation } from './conversations'

export default function ConversationHistory({
  conversations,
  activeId,
  onSelect,
}: {
  conversations: Conversation[]
  activeId: string
  onSelect: (id: string) => void
}) {
  const drawer = useRef<HTMLDialogElement>(null)

  return (
    <>
      <button
        className="icon-button"
        aria-label="打开历史对话"
        title="历史对话"
        onClick={() => drawer.current?.showModal()}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M5 7h14M5 12h14M5 17h14" />
        </svg>
      </button>
      <dialog
        ref={drawer}
        className="history-drawer"
        aria-labelledby="history-title"
        onClick={(event) => {
          if (event.target === event.currentTarget) drawer.current?.close()
        }}
      >
        <div className="history-content">
          <div className="history-heading">
            <h2 id="history-title">历史对话</h2>
            <button
              className="icon-button"
              aria-label="关闭历史对话"
              onClick={() => drawer.current?.close()}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="m6 6 12 12M6 18 18 6" />
              </svg>
            </button>
          </div>
          <nav aria-label="历史对话">
            <ul>
              {[...conversations]
                .sort((a, b) => b.updatedAt - a.updatedAt)
                .map((conversation) => (
                  <li key={conversation.id}>
                    <button
                      className="history-item"
                      aria-current={conversation.id === activeId ? 'page' : undefined}
                      onClick={() => {
                        onSelect(conversation.id)
                        drawer.current?.close()
                      }}
                    >
                      <span>{conversation.title}</span>
                      <time dateTime={new Date(conversation.updatedAt).toISOString()}>
                        {new Date(conversation.updatedAt).toLocaleDateString('zh-CN', {
                          month: 'short',
                          day: 'numeric',
                        })}
                      </time>
                    </button>
                  </li>
                ))}
            </ul>
          </nav>
        </div>
      </dialog>
    </>
  )
}
