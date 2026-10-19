import { useEffect, useState } from 'react'
import { Terminal, ChevronDown, ChevronRight } from 'lucide-react'
import { invoke } from '@tauri-apps/api/tauri'
import { listen } from '@tauri-apps/api/event'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'

interface MessageLogEntry {
  timestamp: string
  log_type: 'rest' | 'grpc'
  message_type: string
  content: string
  status_code?: number
  request_body?: string
  response_body?: string
  duration_ms?: number
}

export function DevLogsPage() {
  const [logs, setLogs] = useState<MessageLogEntry[]>([])
  const [expandedRows, setExpandedRows] = useState<Set<number>>(new Set())
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    // Load initial logs
    loadLogs()

    // Listen for new log entries
    const unlisten = listen<MessageLogEntry>('new-message', (event) => {
      setLogs((prev) => [...prev, event.payload])
    })

    return () => {
      unlisten.then((fn) => fn())
    }
  }, [])

  const loadLogs = async () => {
    try {
      setLoading(true)
      const allLogs = await invoke<MessageLogEntry[]>('get_message_log')
      setLogs(allLogs)
    } catch (err) {
      console.error('Failed to load logs:', err)
    } finally {
      setLoading(false)
    }
  }

  const toggleRow = (index: number) => {
    const newExpanded = new Set(expandedRows)
    if (newExpanded.has(index)) {
      newExpanded.delete(index)
    } else {
      newExpanded.add(index)
    }
    setExpandedRows(newExpanded)
  }

  const formatTimestamp = (timestamp: string): string => {
    const date = new Date(timestamp)
    const time = date.toLocaleTimeString('en-US', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    })
    const ms = String(date.getMilliseconds()).padStart(3, '0')
    return `${time}.${ms}`
  }

  const getLogTypeBadge = (logType: 'rest' | 'grpc') => {
    return logType === 'rest' ? (
      <Badge variant="outline" className="border-blue-500/40 bg-blue-500/10 text-blue-400">
        REST
      </Badge>
    ) : (
      <Badge variant="outline" className="border-purple-500/40 bg-purple-500/10 text-purple-400">
        gRPC
      </Badge>
    )
  }

  const getStatusBadge = (statusCode?: number) => {
    if (!statusCode) return null

    let variant: 'default' | 'destructive' | 'outline' = 'outline'
    if (statusCode >= 200 && statusCode < 300) {
      variant = 'default'
    } else if (statusCode >= 400) {
      variant = 'destructive'
    }

    return (
      <Badge variant={variant} className="font-mono">
        {statusCode}
      </Badge>
    )
  }

  const formatBody = (body?: string): string => {
    if (!body) return 'N/A'
    try {
      const parsed = JSON.parse(body)
      return JSON.stringify(parsed, null, 2)
    } catch {
      return body
    }
  }

  return (
    <div className="container mx-auto p-6 space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <div className="flex items-center gap-2">
            <Terminal className="h-8 w-8" />
            <h1 className="text-3xl font-bold tracking-tight">Dev Logs</h1>
            <Badge variant="secondary">Development Only</Badge>
          </div>
          <p className="text-muted-foreground mt-2">
            Real-time monitoring of REST API calls and gRPC messages
          </p>
        </div>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>API Activity</CardTitle>
          <CardDescription>
            View all REST API requests and gRPC messages in real-time
          </CardDescription>
        </CardHeader>
        <CardContent>
          {loading ? (
            <div className="flex items-center justify-center py-8">
              <div className="text-muted-foreground">Loading logs...</div>
            </div>
          ) : logs.length === 0 ? (
            <div className="text-center py-12">
              <Terminal className="h-12 w-12 mx-auto text-muted-foreground mb-4" />
              <p className="text-muted-foreground">
                No logs yet. Make API calls or connect gRPC to see activity here.
              </p>
            </div>
          ) : (
            <div className="border rounded-lg">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-[40px]"></TableHead>
                    <TableHead>Time</TableHead>
                    <TableHead>Type</TableHead>
                    <TableHead>Method</TableHead>
                    <TableHead>Endpoint</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead className="text-right">Duration</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {logs.map((log, index) => ({ log, index })).reverse().map(({ log, index }) => (
                    <>
                      <TableRow
                        key={index}
                        className="cursor-pointer hover:bg-muted/50"
                        onClick={() => toggleRow(index)}
                      >
                        <TableCell>
                          {expandedRows.has(index) ? (
                            <ChevronDown className="h-4 w-4" />
                          ) : (
                            <ChevronRight className="h-4 w-4" />
                          )}
                        </TableCell>
                        <TableCell className="font-mono text-xs">
                          {formatTimestamp(log.timestamp)}
                        </TableCell>
                        <TableCell>{getLogTypeBadge(log.log_type)}</TableCell>
                        <TableCell>
                          <Badge variant="secondary" className="font-mono">
                            {log.message_type}
                          </Badge>
                        </TableCell>
                        <TableCell className="font-mono text-sm">{log.content}</TableCell>
                        <TableCell>{getStatusBadge(log.status_code)}</TableCell>
                        <TableCell className="text-right font-mono text-xs">
                          {log.duration_ms ? `${log.duration_ms}ms` : '-'}
                        </TableCell>
                      </TableRow>
                      {expandedRows.has(index) && (
                        <TableRow key={`${index}-detail`}>
                          <TableCell colSpan={7} className="bg-muted/30">
                            <div className="space-y-4 p-4">
                              <div className="grid grid-cols-2 gap-4">
                                {/* Request Body */}
                                <div className="space-y-2">
                                  <div className="text-sm font-semibold">Request Body</div>
                                  <pre className="bg-background border rounded p-3 text-xs overflow-auto max-h-60">
                                    {formatBody(log.request_body)}
                                  </pre>
                                </div>

                                {/* Response Body */}
                                <div className="space-y-2">
                                  <div className="text-sm font-semibold">Response Body</div>
                                  <pre className="bg-background border rounded p-3 text-xs overflow-auto max-h-60">
                                    {formatBody(log.response_body)}
                                  </pre>
                                </div>
                              </div>

                              {/* Full timestamp */}
                              <div className="text-xs text-muted-foreground">
                                Full timestamp: {log.timestamp}
                              </div>
                            </div>
                          </TableCell>
                        </TableRow>
                      )}
                    </>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
