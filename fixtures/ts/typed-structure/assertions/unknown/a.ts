declare const broad: unknown[]; broad as string[]; declare const tuple: [unknown]; tuple as [number]; function opaque<T, K>(value: T[K]) { value as string; }
