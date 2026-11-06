<template>
  <section class="task-list">
    <header class="task-list__header">
      <h2 class="task-list__title">{{ title }}</h2>
      <span class="task-list__count">{{ openCount }} open</span>
    </header>
    <form class="task-list__form" @submit.prevent="addTask">
      <input
        v-model="draft"
        class="task-list__input"
        type="text"
        placeholder="New task"
      />
      <button class="task-list__add" type="submit">Add</button>
    </form>
    <ul class="task-list__items">
      <li
        v-for="task in visibleTasks"
        :key="task.id"
        class="task-list__item"
        :class="{ 'task-list__item--done': task.done }"
      >
        <label class="task-list__label">
          <input type="checkbox" :checked="task.done" @change="toggleTask(task.id)" />
          <span class="task-list__text">{{ task.text }}</span>
        </label>
        <button class="task-list__remove" type="button" @click="removeTask(task.id)">
          Remove
        </button>
      </li>
    </ul>
    <p v-if="!visibleTasks.length" class="task-list__empty">Nothing to do.</p>
  </section>
</template>

<script>
export default {
  name: 'TaskList',
  props: {
    title: { type: String, default: 'Tasks' },
    initialTasks: { type: Array, default: () => [] },
    hideDone: { type: Boolean, default: false },
  },
  data() {
    return {
      draft: '',
      tasks: this.initialTasks.slice(),
      nextId: this.initialTasks.length + 1,
    }
  },
  computed: {
    visibleTasks() {
      return this.hideDone ? this.tasks.filter((t) => !t.done) : this.tasks
    },
    openCount() {
      return this.tasks.filter((t) => !t.done).length
    },
  },
  methods: {
    addTask() {
      const text = this.draft.trim()
      if (!text) {
        return
      }
      this.tasks.push({ id: this.nextId, text, done: false })
      this.nextId += 1
      this.draft = ''
      this.$emit('added', text)
    },
    toggleTask(id) {
      const task = this.tasks.find((t) => t.id === id)
      if (task) {
        task.done = !task.done
        this.$emit('toggled', { id, done: task.done })
      }
    },
    removeTask(id) {
      this.tasks = this.tasks.filter((t) => t.id !== id)
      this.$emit('removed', id)
    },
    clearDone() {
      const removed = this.tasks.filter((t) => t.done).length
      this.tasks = this.tasks.filter((t) => !t.done)
      this.$emit('cleared', removed)
    },
  },
}
</script>

<style scoped>
.task-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  border: 1px solid #d0d4da;
  border-radius: 8px;
  background: #ffffff;
}
.task-list__header {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
}
.task-list__title {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
}
.task-list__count {
  color: #5b6573;
  font-size: 13px;
}
.task-list__form {
  display: flex;
  gap: 8px;
}
.task-list__input {
  flex: 1;
  padding: 6px 10px;
  border: 1px solid #b8bec7;
  border-radius: 4px;
}
.task-list__add {
  padding: 6px 14px;
  border: 0;
  border-radius: 4px;
  background: #2f6fed;
  color: #ffffff;
  cursor: pointer;
}
.task-list__items {
  margin: 0;
  padding: 0;
  list-style: none;
}
.task-list__item {
  display: flex;
  justify-content: space-between;
  padding: 8px 0;
  border-bottom: 1px solid #eceef1;
}
.task-list__item--done .task-list__text {
  color: #8a93a0;
  text-decoration: line-through;
}
.task-list__remove {
  border: 0;
  background: transparent;
  color: #c0392b;
  cursor: pointer;
}
.task-list__empty {
  margin: 0;
  color: #5b6573;
  font-style: italic;
}
</style>
