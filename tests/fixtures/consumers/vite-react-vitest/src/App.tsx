import { useReducer } from "react";

export function increment(value: number): number {
  return value + 1;
}

export type CounterAction = "increment" | "reset";

export function counterReducer(value: number, action: CounterAction): number {
  switch (action) {
    case "increment":
      return increment(value);
    case "reset":
      return 0;
  }
}

export function App() {
  const [count, dispatch] = useReducer(counterReducer, 0);
  return (
    <button onClick={() => dispatch("increment")}>
      {count === 0 ? "Start" : `Count: ${count}`}
    </button>
  );
}
