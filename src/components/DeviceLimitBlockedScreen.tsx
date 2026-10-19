export function DeviceLimitBlockedScreen({ message }: { message: string }) {
  return (
    <div className="flex h-screen items-center justify-center">
      <div className="text-center max-w-md">
        <div className="text-destructive mb-4">
          <svg className="h-12 w-12 mx-auto" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
          </svg>
        </div>
        <h2 className="text-xl font-bold mb-2">Device Limit Reached</h2>
        <p className="text-muted-foreground mb-4">{message}</p>
        <p className="text-sm text-muted-foreground">
          Contact{' '}
          <a href="mailto:hello@getnodea.com" className="text-primary underline">
            hello@getnodea.com
          </a>{' '}
          to increase your device limit.
        </p>
      </div>
    </div>
  )
}
