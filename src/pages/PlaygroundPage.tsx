import { useEffect, useState, useRef } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { toast } from 'sonner'
import { Send, Bot, User, Loader2 } from 'lucide-react'
import { Card, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import { apiClient } from '@/lib/api'
import { StreamingMarkdown } from '@/components/StreamingMarkdown'

interface Message {
  id: string
  role: 'user' | 'assistant'
  content: string
  timestamp: Date
}

interface Model {
  id: string
  name: string
}

export function PlaygroundPage() {
  const [models, setModels] = useState<Model[]>([])
  const [selectedModel, setSelectedModel] = useState<string>('')
  const [messages, setMessages] = useState<Message[]>([])
  const [input, setInput] = useState('')
  const [isStreaming, setIsStreaming] = useState(false)
  const [streamingContent, setStreamingContent] = useState('')

  const messagesEndRef = useRef<HTMLDivElement>(null)
  const eventSourceRef = useRef<EventSource | null>(null)

  useEffect(() => {
    console.log('[Playground] Connecting to models stream...')
    const modelsStream = apiClient.createModelsStream()

    modelsStream.onopen = () => {
      console.log('[Playground] Models stream connected')
    }

    modelsStream.onmessage = (event) => {
      console.log('[Playground] Received models stream event:', event.data)
      try {
        const data = JSON.parse(event.data)
        console.log('[Playground] Parsed data:', data)
        if (data.type === 'available_models' && Array.isArray(data.value)) {
          console.log('[Playground] Available models:', data.value)
          const modelList = data.value.map((modelName: string) => ({
            id: modelName,
            name: modelName,
          }))
          setModels(modelList)
          if (modelList.length > 0 && !selectedModel) {
            setSelectedModel(modelList[0].id)
          }
        }
      } catch (err) {
        console.error('[Playground] Failed to parse models:', err)
      }
    }

    modelsStream.onerror = (error) => {
      console.error('[Playground] Models stream error:', error)
    }

    return () => {
      console.log('[Playground] Closing models stream')
      modelsStream.close()
      if (eventSourceRef.current) {
        eventSourceRef.current.close()
      }
    }
  }, [])

  useEffect(() => {
    scrollToBottom()
  }, [messages, streamingContent])

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }

  const logFrontendEvent = (level: 'info' | 'warn' | 'error', message: string) => {
    invoke('log_frontend_event', { level, message }).catch((err: unknown) => {
      console.error('Failed to log frontend event:', err)
    })
  }

  const handleSend = async () => {
    if (!input.trim() || !selectedModel || isStreaming) {
      logFrontendEvent(
        'warn',
        `[Playground] handleSend blocked: input=${JSON.stringify(input.trim())} selectedModel=${JSON.stringify(selectedModel)} isStreaming=${isStreaming}`
      )
      return
    }

    const userMessage: Message = {
      id: Date.now().toString(),
      role: 'user',
      content: input.trim(),
      timestamp: new Date(),
    }

    setMessages((prev) => [...prev, userMessage])
    setInput('')
    setIsStreaming(true)
    setStreamingContent('')

    try {
      const response = await apiClient.createCompletion({
        text: userMessage.content,
        model: selectedModel,
      })

      const streamSource = apiClient.createStreamEventSource(response.id)
      eventSourceRef.current = streamSource

      streamSource.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data)

          if (data.type === 'content') {
            setStreamingContent((prev) => prev + data.text)
          } else if (data.type === 'done') {
            const assistantMessage: Message = {
              id: response.id,
              role: 'assistant',
              content: data.full_text || streamingContent,
              timestamp: new Date(),
            }
            setMessages((prev) => [...prev, assistantMessage])
            setStreamingContent('')
            setIsStreaming(false)
            streamSource.close()
            eventSourceRef.current = null
          } else if (data.type === 'error') {
            toast.error(data.message || 'The model did not respond in time. Please try again.')
            logFrontendEvent('error', `[Playground] Completion stream reported an error for request ${response.id}: ${data.message}`)
            setIsStreaming(false)
            setStreamingContent('')
            streamSource.close()
            eventSourceRef.current = null
          }
        } catch (err) {
          console.error('Failed to parse stream data:', err)
          logFrontendEvent('error', `[Playground] Failed to parse stream data: ${String(err)}`)
        }
      }

      streamSource.onerror = (error) => {
        console.error('Stream error:', error)
        logFrontendEvent('error', `[Playground] Completion stream error for request ${response.id}`)
        toast.error('Connection to the model was lost. Please try again.')
        setIsStreaming(false)
        setStreamingContent('')
        streamSource.close()
        eventSourceRef.current = null
      }
    } catch (err) {
      console.error('Failed to create completion:', err)
      logFrontendEvent('error', `[Playground] Failed to create completion: ${String(err)}`)
      toast.error('Failed to send message. Please try again.')
      setIsStreaming(false)
      setStreamingContent('')
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  return (
    <div className="container mx-auto p-6 h-[calc(100vh-6rem)]">
      <div className="flex flex-col h-full space-y-4">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-3xl font-bold tracking-tight">AI Playground</h1>
            <p className="text-muted-foreground mt-2">
              Test and interact with AI models
            </p>
          </div>

          <div className="flex items-center gap-2">
            <span className="text-sm text-muted-foreground">Model:</span>
            <Select value={selectedModel} onValueChange={setSelectedModel}>
              <SelectTrigger className="w-[200px]">
                <SelectValue placeholder="Select model" />
              </SelectTrigger>
              <SelectContent>
                {models.map((model) => (
                  <SelectItem key={model.id} value={model.id}>
                    {model.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>

        <Card className="flex-1 flex flex-col">
          <CardContent className="flex-1 flex flex-col p-0">
            <div className="flex-1 overflow-y-auto p-6 space-y-4">
              {messages.length === 0 ? (
                <div className="flex items-center justify-center h-full">
                  <div className="text-center space-y-3">
                    <Bot className="h-12 w-12 mx-auto text-muted-foreground" />
                    <h3 className="text-lg font-semibold">Start a conversation</h3>
                    <p className="text-sm text-muted-foreground max-w-sm">
                      Type a message below to start chatting with the AI model
                    </p>
                  </div>
                </div>
              ) : (
                <>
                  {messages.map((message) => (
                    <div key={message.id} className="flex gap-3">
                      <div
                        className={`flex h-8 w-8 shrink-0 select-none items-center justify-center rounded-md ${
                          message.role === 'user'
                            ? 'bg-primary text-primary-foreground'
                            : 'bg-muted'
                        }`}
                      >
                        {message.role === 'user' ? (
                          <User className="h-4 w-4" />
                        ) : (
                          <Bot className="h-4 w-4" />
                        )}
                      </div>
                      <div className="flex-1 space-y-2">
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-semibold">
                            {message.role === 'user' ? 'You' : 'AI'}
                          </span>
                          <span className="text-xs text-muted-foreground">
                            {message.timestamp.toLocaleTimeString()}
                          </span>
                        </div>
                        <div className="text-sm break-words">
                          {message.role === 'user' ? (
                            <div className="whitespace-pre-wrap">{message.content}</div>
                          ) : (
                            <StreamingMarkdown content={message.content} />
                          )}
                        </div>
                      </div>
                    </div>
                  ))}

                  {isStreaming && (
                    <div className="flex gap-3">
                      <div className="flex h-8 w-8 shrink-0 select-none items-center justify-center rounded-md bg-muted">
                        <Bot className="h-4 w-4" />
                      </div>
                      <div className="flex-1 space-y-2">
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-semibold">AI</span>
                          <Badge variant="secondary" className="h-5">
                            <Loader2 className="h-3 w-3 mr-1 animate-spin" />
                            Typing...
                          </Badge>
                        </div>
                        <div className="text-sm break-words">
                          <StreamingMarkdown content={streamingContent} />
                          <span className="inline-block w-2 h-4 bg-foreground animate-pulse ml-1" />
                        </div>
                      </div>
                    </div>
                  )}

                  <div ref={messagesEndRef} />
                </>
              )}
            </div>

            <Separator />

            <div className="p-4">
              <div className="flex gap-2">
                <Input
                  placeholder="Type your message..."
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  onKeyDown={handleKeyDown}
                  disabled={isStreaming || !selectedModel}
                />
                <Button
                  onClick={handleSend}
                  disabled={!input.trim() || isStreaming || !selectedModel}
                  size="icon"
                >
                  {isStreaming ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Send className="h-4 w-4" />
                  )}
                </Button>
              </div>
              {!selectedModel && (
                <p className="text-xs text-muted-foreground mt-2">
                  Waiting for available models...
                </p>
              )}
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
