export interface Action { id: string; title: string }
export interface ListItem { id: string; title: string; subtitle?: string; icon?: string; actions?: Action[] }
export interface Field { id: string; label: string; type?: 'text' | 'password' | 'boolean'; value?: string | boolean; placeholder?: string; required?: boolean }
export type View = { type: 'list'; items: ListItem[]; title?: string } | { type: 'detail'; title: string; text: string } | { type: 'form'; title: string; fields: Field[]; submit_label?: string };
export interface Context { command: string; query: string; signal: AbortSignal; item?: string; action?: string; values?: Record<string, string | boolean> }
export interface Command { run(context: Context): View | null | void | Promise<View | null | void>; query?: Command['run']; action?: Command['run']; submit?: Command['run'] }
export function serve(commands: Record<string, Command>): void;
export function update(view: View): void;
export function list(items: ListItem[], title?: string): View;
export function detail(title: string, text: string): View;
export function form(title: string, fields: Field[], submit_label?: string): View;
export const host: { copy(text: string): Promise<void>; open(url: string): Promise<void>; notify(text: string): Promise<void>; storage: {get<T = unknown>(key: string): Promise<T | null>; set(key: string, value: unknown): Promise<void>} };
