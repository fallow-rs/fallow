import { createPinia } from "pinia";
import { Counter } from "./counter";
import { useCounterStore } from "./store";

const counter = new Counter();
counter.increment();
console.log(counter.read());

const store = useCounterStore(createPinia());
console.log(store.doubled);
