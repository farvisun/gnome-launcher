export interface Action { id: string; title: string }
export type Color = 'red' | 'orange' | 'yellow' | 'green' | 'blue' | 'purple' | 'accent' | 'dim';
export interface Accessory { text?: string; tag?: string; icon?: string; color?: Color; tooltip?: string }
export interface MetadataRow { label?: string; text?: string; color?: Color; tags?: {text: string; color?: Color}[]; separator?: boolean }
export interface ItemDetail {
  hero?: {value: string; unit?: string; caption?: string; progress?: number; color?: Color};
  chart?: {values: number[]; caption?: string; color?: Color};
  text?: string;
  metadata?: MetadataRow[];
}
export interface ListItem { id: string; title: string; subtitle?: string; icon?: string; tint?: Color; section?: string; progress?: number; accessories?: Accessory[]; detail?: ItemDetail; actions?: Action[] }
export interface ListOptions { title?: string; placeholder?: string; detail?: boolean; refresh?: number; empty?: {title: string; description?: string; icon?: string} }
export interface Field { id: string; label: string; type?: 'text' | 'password' | 'boolean'; value?: string | boolean; placeholder?: string; required?: boolean }
export type View = ({ type: 'list'; items: ListItem[] } & ListOptions) | { type: 'detail'; title: string; text: string } | { type: 'form'; title: string; fields: Field[]; submit_label?: string };
export interface Context { command: string; query: string; signal: AbortSignal; item?: string; action?: string; values?: Record<string, string | boolean> }
export interface Command { run(context: Context): View | null | void | Promise<View | null | void>; query?: Command['run']; action?: Command['run']; submit?: Command['run'] }
export function serve(commands: Record<string, Command>): void;
export function update(view: View): void;
export function list(items: ListItem[], options?: string | ListOptions): View;
export function detail(title: string, text: string): View;
export function form(title: string, fields: Field[], submit_label?: string): View;
export const host: { copy(text: string): Promise<void>; open(url: string): Promise<void>; notify(text: string): Promise<void>; storage: {get<T = unknown>(key: string): Promise<T | null>; set(key: string, value: unknown): Promise<void>} };
