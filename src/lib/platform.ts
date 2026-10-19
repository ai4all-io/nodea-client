/**
 * Platform detection utilities
 * Determines if the app is running in Tauri (desktop) or web mode
 */

export const isTauriApp = (): boolean => {
  return typeof window !== 'undefined' && '__TAURI__' in window
}

export const isWebMode = (): boolean => {
  return !isTauriApp()
}

/**
 * Platform-specific configuration
 */
export const getPlatformInfo = () => {
  const tauri = isTauriApp()
  return {
    isTauri: tauri,
    isWeb: !tauri,
    name: tauri ? 'Tauri Desktop' : 'Web Browser',
  }
}
