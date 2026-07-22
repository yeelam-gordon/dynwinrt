'use strict';
let activeScope = null;
function trackProjectedValue(value, typeName) {
activeScope?.track(value, typeName);
return value;
}
function createProjectedLifetimeScope() {
const previousScope = activeScope;
const registry = { values: [], nextSweep: 1024 };
let disposed = false;
const scope = {
get disposed() { return disposed; },
track(value, typeName) {
if (disposed) throw new Error('Cannot track values in a disposed projection scope.');
registry.values.push({ ref: new WeakRef(value), typeName });
if (registry.values.length >= registry.nextSweep) {
registry.values = registry.values.filter((entry) => entry.ref.deref() !== undefined);
registry.nextSweep = Math.max(registry.values.length * 2, 1024);
}
},
dispose() {
if (disposed) return;
if (activeScope !== scope) throw new Error('Projection lifetime scopes must be disposed in LIFO order.');
const retained = [];
let firstError;
for (const entry of [...registry.values].reverse()) {
const value = entry.ref.deref();
if (value === undefined) continue;
try { value.release(); }
catch (error) { firstError ??= error; retained.push(entry); }
}
registry.values = retained.reverse();
registry.nextSweep = Math.max(registry.values.length * 2, 1024);
if (firstError !== undefined) throw firstError;
disposed = true;
activeScope = previousScope;
},
};
activeScope = scope;
return scope;
}
exports.trackProjectedValue = trackProjectedValue;
exports.createProjectedLifetimeScope = createProjectedLifetimeScope;
