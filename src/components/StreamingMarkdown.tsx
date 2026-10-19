import { useMemo } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter'
import { vscDarkPlus } from 'react-syntax-highlighter/dist/esm/styles/prism'

interface ParsedContent {
  type: 'complete' | 'open-code'
  content?: string
  beforeCode?: string
  code?: {
    lang: string
    content: string
  }
}

/**
 * Parses streaming content to detect open/closed code blocks
 */
function parseStreamingContent(content: string): ParsedContent {
  const codeBlockDelimiters: number[] = []
  const regex = /```/g
  let match

  // Find all ``` occurrences
  while ((match = regex.exec(content)) !== null) {
    codeBlockDelimiters.push(match.index)
  }

  // If odd number of delimiters, we have an open code block
  if (codeBlockDelimiters.length % 2 === 1) {
    const lastDelimiterIndex = codeBlockDelimiters[codeBlockDelimiters.length - 1]

    // Extract language if present
    const afterDelimiter = content.substring(lastDelimiterIndex + 3)
    const firstNewline = afterDelimiter.indexOf('\n')

    let lang = ''
    let codeStartOffset = 0

    if (firstNewline !== -1 && firstNewline < 20) {
      // Language specified (e.g., ```python)
      const potentialLang = afterDelimiter.substring(0, firstNewline).trim()
      if (potentialLang && !/\s/.test(potentialLang)) {
        lang = potentialLang
        codeStartOffset = firstNewline + 1
      }
    }

    return {
      type: 'open-code',
      beforeCode: content.substring(0, lastDelimiterIndex),
      code: {
        lang,
        content: afterDelimiter.substring(codeStartOffset),
      },
    }
  }

  // All code blocks are closed, render normally
  return {
    type: 'complete',
    content,
  }
}

interface StreamingMarkdownProps {
  content: string
}

/**
 * StreamingMarkdown component that renders markdown in real-time,
 * including incomplete code blocks with syntax highlighting
 */
export function StreamingMarkdown({ content }: StreamingMarkdownProps) {
  const parsed = useMemo(() => parseStreamingContent(content), [content])

  const markdownComponents = {
    code({ node, inline, className, children, ...props }: any) {
      const match = /language-(\w+)/.exec(className || '')
      const lang = match ? match[1] : ''
      const codeString = String(children).replace(/\n$/, '')

      if (!inline && codeString) {
        return (
          <SyntaxHighlighter
            style={vscDarkPlus}
            language={lang || 'text'}
            PreTag="div"
            className="rounded-lg my-4"
            {...props}
          >
            {codeString}
          </SyntaxHighlighter>
        )
      }

      // Inline code
      return (
        <code
          className="bg-muted px-1.5 py-0.5 rounded text-sm font-mono"
          {...props}
        >
          {children}
        </code>
      )
    },
    h1: ({ children }: any) => (
      <h1 className="text-3xl font-bold mt-6 mb-4">{children}</h1>
    ),
    h2: ({ children }: any) => (
      <h2 className="text-2xl font-bold mt-5 mb-3">{children}</h2>
    ),
    h3: ({ children }: any) => (
      <h3 className="text-xl font-bold mt-4 mb-2">{children}</h3>
    ),
    p: ({ children }: any) => <p className="mb-4 leading-7">{children}</p>,
    ul: ({ children }: any) => (
      <ul className="list-disc list-inside mb-4 space-y-1">{children}</ul>
    ),
    ol: ({ children }: any) => (
      <ol className="list-decimal list-inside mb-4 space-y-1">{children}</ol>
    ),
    blockquote: ({ children }: any) => (
      <blockquote className="border-l-4 border-muted-foreground pl-4 italic my-4 text-muted-foreground">
        {children}
      </blockquote>
    ),
    a: ({ href, children }: any) => (
      <a
        href={href}
        className="text-primary hover:underline"
        target="_blank"
        rel="noopener noreferrer"
      >
        {children}
      </a>
    ),
  }

  if (parsed.type === 'complete') {
    // All code blocks are closed, render normally
    return (
      <div className="streaming-markdown">
        <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
          {parsed.content}
        </ReactMarkdown>
      </div>
    )
  }

  // We have an open code block
  const { beforeCode, code } = parsed

  return (
    <div className="streaming-markdown">
      {beforeCode && (
        <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
          {beforeCode}
        </ReactMarkdown>
      )}
      {code && code.content !== undefined && (
        <SyntaxHighlighter
          style={vscDarkPlus}
          language={code.lang || 'text'}
          PreTag="div"
          className="rounded-lg my-4"
        >
          {code.content}
        </SyntaxHighlighter>
      )}
    </div>
  )
}
