import { useState } from "react";

export function increment(value: number): number {
  return value + 1;
}

export function App() {
  const [count, setCount] = useState(0);
  return <button onClick={() => setCount(increment(count))}>{count}</button>;
}
