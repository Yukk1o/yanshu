const state = {
  tasks: [],
  filter: "all",
  editingId: null,
  busyIds: new Set(),
};

const elements = {
  form: document.querySelector("#task-form"),
  title: document.querySelector("#task-title"),
  list: document.querySelector("#task-list"),
  refresh: document.querySelector("#refresh-button"),
  filters: [...document.querySelectorAll("[data-filter]")],
  total: document.querySelector("#total-count"),
  open: document.querySelector("#open-count"),
  done: document.querySelector("#done-count"),
  progress: document.querySelector("#progress-bar"),
  progressCopy: document.querySelector("#progress-copy"),
  runtimeStatus: document.querySelector("#runtime-status"),
  runtimeStatusText: document.querySelector("#runtime-status-text"),
  toast: document.querySelector("#toast"),
};

let toastTimer;

async function api(path, options = {}) {
  const response = await fetch(path, {
    ...options,
    headers: options.body
      ? { "Content-Type": "application/json", ...(options.headers || {}) }
      : options.headers,
  });
  const payload = await response.json().catch(() => null);
  if (!response.ok) {
    const error = new Error(
      payload?.error?.message || `请求失败（HTTP ${response.status}）`,
    );
    error.code = payload?.error?.code || "HTTP_ERROR";
    throw error;
  }
  return payload;
}

async function loadTasks({ quiet = false } = {}) {
  if (!quiet) {
    elements.refresh.classList.add("loading");
    elements.list.setAttribute("aria-busy", "true");
  }

  try {
    const tasks = await api("/tasks");
    state.tasks = Array.isArray(tasks) ? tasks : [];
    setRuntimeStatus(true);
    render();
  } catch (error) {
    setRuntimeStatus(false);
    renderError(error);
  } finally {
    elements.refresh.classList.remove("loading");
    elements.list.setAttribute("aria-busy", "false");
  }
}

async function createTask(title) {
  const id = makeTaskId();
  const submitButton = elements.form.querySelector("button[type='submit']");
  submitButton.disabled = true;

  try {
    const created = await api("/tasks", {
      method: "POST",
      body: JSON.stringify({ id, title, completed: false }),
    });
    state.tasks.push(created);
    elements.form.reset();
    render();
    showToast("任务已写入事务台账");
    elements.title.focus();
  } catch (error) {
    showToast(`${error.code}: ${error.message}`, true);
  } finally {
    submitButton.disabled = false;
  }
}

async function updateTask(id, patch) {
  const current = state.tasks.find((task) => task.id === id);
  if (!current || state.busyIds.has(id)) return;

  state.busyIds.add(id);
  render();
  try {
    const updated = await api(`/tasks/${encodeURIComponent(id)}`, {
      method: "PUT",
      body: JSON.stringify(patch),
    });
    state.tasks = state.tasks.map((task) => (task.id === id ? updated : task));
    state.editingId = null;
    showToast("任务状态已提交");
  } catch (error) {
    showToast(`${error.code}: ${error.message}`, true);
  } finally {
    state.busyIds.delete(id);
    render();
  }
}

async function deleteTask(id) {
  const task = state.tasks.find((item) => item.id === id);
  if (!task || state.busyIds.has(id)) return;
  if (!window.confirm(`确认删除“${task.title}”？`)) return;

  state.busyIds.add(id);
  render();
  try {
    await api(`/tasks/${encodeURIComponent(id)}`, { method: "DELETE" });
    state.tasks = state.tasks.filter((item) => item.id !== id);
    showToast("任务已从台账删除");
  } catch (error) {
    showToast(`${error.code}: ${error.message}`, true);
  } finally {
    state.busyIds.delete(id);
    render();
  }
}

function render() {
  updateStats();
  const visibleTasks = state.tasks.filter((task) => {
    if (state.filter === "open") return !task.completed;
    if (state.filter === "done") return task.completed;
    return true;
  });

  if (visibleTasks.length === 0) {
    const copy = state.tasks.length === 0
      ? ["台账还是空的", "从上方写入第一项任务，验证完整后端链路。"]
      : ["这个筛选下没有任务", "换一个状态，或者创建一项新的工作。"];
    elements.list.innerHTML = `
      <div class="empty-state">
        <strong>${copy[0]}</strong>
        <p>${copy[1]}</p>
      </div>`;
    return;
  }

  elements.list.innerHTML = visibleTasks
    .map((task, index) => taskTemplate(task, index))
    .join("");
}

function taskTemplate(task, index) {
  const busy = state.busyIds.has(task.id);
  const editing = state.editingId === task.id;
  const safeId = escapeHtml(task.id);
  const safeTitle = escapeHtml(task.title);
  const status = task.completed ? "已完成" : "进行中";
  const timestamp = formatTimestamp(task.updatedAt);

  return `
    <article
      class="task-card stagger-${Math.min(index, 5)} ${task.completed ? "completed" : ""}"
      data-id="${safeId}"
      data-index="${String(index + 1).padStart(2, "0")}"
    >
      <input
        class="task-toggle"
        type="checkbox"
        data-action="toggle"
        aria-label="${task.completed ? "标记为未完成" : "标记为已完成"}：${safeTitle}"
        ${task.completed ? "checked" : ""}
        ${busy ? "disabled" : ""}
      >
      <div class="task-content">
        ${editing ? editTemplate(task) : `
          <h3 class="task-title">${safeTitle}</h3>
          <p class="task-meta">
            <span>${status}</span>
            <span>ID ${safeId}</span>
            <span>${timestamp}</span>
          </p>`}
      </div>
      ${editing ? "" : `
        <div class="task-controls">
          <button class="task-action" type="button" data-action="edit" ${busy ? "disabled" : ""}>编辑</button>
          <button class="task-action danger" type="button" data-action="delete" ${busy ? "disabled" : ""}>删除</button>
        </div>`}
    </article>`;
}

function editTemplate(task) {
  return `
    <form class="edit-form" data-edit-form>
      <label class="section-kicker" for="edit-${escapeHtml(task.id)}">EDIT ENTRY</label>
      <input
        class="edit-input"
        id="edit-${escapeHtml(task.id)}"
        name="title"
        maxlength="120"
        value="${escapeHtml(task.title)}"
        required
      >
      <div class="edit-actions">
        <button class="edit-button" type="submit">保存</button>
        <button class="edit-button" type="button" data-action="cancel">取消</button>
      </div>
    </form>`;
}

function updateStats() {
  const total = state.tasks.length;
  const done = state.tasks.filter((task) => task.completed).length;
  const open = total - done;
  const percentage = total === 0 ? 0 : Math.round((done / total) * 100);

  elements.total.textContent = String(total).padStart(2, "0");
  elements.open.textContent = String(open).padStart(2, "0");
  elements.done.textContent = String(done).padStart(2, "0");
  elements.progress.style.width = `${percentage}%`;
  elements.progressCopy.textContent = total === 0
    ? "等待第一条运行数据。"
    : `完成度 ${percentage}% · ${open} 项仍在推进`;
}

function renderError(error) {
  elements.list.innerHTML = `
    <div class="error-state">
      <strong>暂时连不上后端</strong>
      <p>${escapeHtml(error.message)}。确认 serve-tasks.ps1 正在运行后重试。</p>
    </div>`;
  elements.total.textContent = "—";
  elements.open.textContent = "—";
  elements.done.textContent = "—";
}

function setRuntimeStatus(online) {
  elements.runtimeStatus.classList.toggle("online", online);
  elements.runtimeStatus.classList.toggle("offline", !online);
  elements.runtimeStatusText.textContent = online ? "解释器在线" : "运行时离线";
}

function showToast(message, isError = false) {
  window.clearTimeout(toastTimer);
  elements.toast.textContent = message;
  elements.toast.classList.toggle("error", isError);
  elements.toast.classList.add("visible");
  toastTimer = window.setTimeout(() => {
    elements.toast.classList.remove("visible");
  }, 2800);
}

function makeTaskId() {
  const randomPart = globalThis.crypto?.randomUUID
    ? globalThis.crypto.randomUUID().slice(0, 8)
    : Math.random().toString(16).slice(2, 10);
  return `${Date.now().toString(36)}-${randomPart}`;
}

function formatTimestamp(value) {
  if (!Number.isFinite(value)) return "时间未知";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

elements.form.addEventListener("submit", (event) => {
  event.preventDefault();
  const title = elements.title.value.trim();
  if (title) createTask(title);
});

elements.refresh.addEventListener("click", () => loadTasks());

elements.filters.forEach((button) => {
  button.addEventListener("click", () => {
    state.filter = button.dataset.filter;
    elements.filters.forEach((item) => item.classList.toggle("active", item === button));
    render();
  });
});

elements.list.addEventListener("change", (event) => {
  if (!event.target.matches("[data-action='toggle']")) return;
  const card = event.target.closest("[data-id]");
  updateTask(card.dataset.id, { completed: event.target.checked });
});

elements.list.addEventListener("click", (event) => {
  const button = event.target.closest("[data-action]");
  if (!button) return;
  const card = button.closest("[data-id]");
  const id = card?.dataset.id;

  if (button.dataset.action === "edit") {
    state.editingId = id;
    render();
    document.querySelector(`#edit-${CSS.escape(id)}`)?.focus();
  } else if (button.dataset.action === "cancel") {
    state.editingId = null;
    render();
  } else if (button.dataset.action === "delete") {
    deleteTask(id);
  }
});

elements.list.addEventListener("submit", (event) => {
  if (!event.target.matches("[data-edit-form]")) return;
  event.preventDefault();
  const card = event.target.closest("[data-id]");
  const title = new FormData(event.target).get("title")?.trim();
  if (title) updateTask(card.dataset.id, { title });
});

loadTasks();
