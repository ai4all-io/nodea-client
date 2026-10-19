/**
 * Where the desktop app sends people when it hands off to the web app.
 *
 * The console lives under /console - the bare origin is the marketing and
 * auth surface, so linking there bounced signed-in users through a redirect
 * instead of landing them on the thing the link promised.
 */
export const WEB_CONSOLE_URL = 'https://app.getnodea.com/console/'
