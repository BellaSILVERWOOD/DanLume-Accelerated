<script setup lang="ts">
// 备忘：左边列表、右边富文本编辑器（对应旧版 page_memos.py）
import { computed, onMounted, ref, watch } from "vue";
import SoftButton from "../components/SoftButton.vue";
import IconGlyph from "../components/IconGlyph.vue";
import {
  addMemo,
  deleteMemo,
  memos,
  updateMemo,
  type MemoRow,
} from "../api";

const rows = ref<MemoRow[]>([]);
const keyword = ref("");
const current = ref<MemoRow | null>(null);
const title = ref("");
const status = ref("");
const editor = ref<HTMLElement | null>(null);
const saving = ref(false);
let dirty = false;
let timer: number | undefined;

const visible = computed(() => {
  const text = keyword.value.trim().toLowerCase();
  if (!text) return rows.value;
  return rows.value.filter(
    (row) =>
      row.title.toLowerCase().includes(text) ||
      row.html.toLowerCase().includes(text),
  );
});

function snippet(html: string): string {
  // 先扔掉 <style>/<script> 整块内容（以前会把 p, li { white-space… } 当正文显示）
  const text = String(html || "")
    .replace(/<style[\s\S]*?<\/style>/gi, " ")
    .replace(/<script[\s\S]*?<\/script>/gi, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&nbsp;/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  return text.slice(0, 40) || "（空白）";
}

async function load(selectFirst = true) {
  rows.value = await memos();
  if (selectFirst && rows.value.length && !current.value) {
    select(rows.value[0]);
  }
}

function select(row: MemoRow) {
  current.value = row;
  title.value = row.title === "无标题备忘" ? "" : row.title;
  if (editor.value) editor.value.innerHTML = row.html || "";
  status.value = row.updated_at ? `上次保存 ${row.updated_at.slice(11)}` : "";
  dirty = false;
}

function countWords(html: string): number {
  return html.replace(/<[^>]+>/g, "").replace(/\s/g, "").length;
}

const words = computed(() => (editor.value ? countWords(editor.value.innerHTML) : 0));

function exec(command: string, value?: string) {
  editor.value?.focus();
  document.execCommand(command, false, value);
  markDirty();
}

function formatBlock(tag: string) {
  exec("formatBlock", tag);
}

function markDirty() {
  dirty = true;
  status.value = "正在输入…";
  if (timer) window.clearTimeout(timer);
  timer = window.setTimeout(save, 800);
}

async function save() {
  if (!editor.value || saving.value) return;
  const html = editor.value.innerHTML;
  const heading =
    title.value.trim() ||
    html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim().slice(0, 20) ||
    "无标题备忘";
  saving.value = true;
  try {
    if (current.value) {
      await updateMemo(current.value.id, heading, html);
      const now = new Date();
      status.value = `已保存 ${String(now.getHours()).padStart(2, "0")}:${String(
        now.getMinutes(),
      ).padStart(2, "0")}:${String(now.getSeconds()).padStart(2, "0")}`;
      current.value.title = heading;
      current.value.html = html;
      current.value.updated_at = status.value.replace("已保存 ", "");
    } else {
      const id = await addMemo(heading, html);
      await load(false);
      const created = rows.value.find((row) => row.id === id);
      if (created) current.value = created;
      status.value = "已保存（新建）";
    }
    dirty = false;
    await load(false);
  } finally {
    saving.value = false;
  }
}

async function create() {
  const id = await addMemo("", "");
  await load(false);
  const created = rows.value.find((row) => row.id === id);
  if (created) select(created);
  editor.value?.focus();
}

async function remove() {
  if (!current.value) return;
  await deleteMemo(current.value.id);
  current.value = null;
  title.value = "";
  if (editor.value) editor.value.innerHTML = "";
  status.value = "已删除";
  await load();
}

onMounted(async () => {
  await load();
  if (!current.value && rows.value.length) select(rows.value[0]);
});

watch(title, () => {
  if (current.value) markDirty();
});
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">备忘录</h1>
        <div class="muted">随手记：支持加粗、斜体、字号、颜色和待办清单，自动保存</div>
      </div>
      <div class="spacer" />
      <SoftButton variant="primary" :height="34" :font-size="13" glyph="ADD" @click="create">
        新建备忘
      </SoftButton>
      <SoftButton :height="34" :font-size="13" glyph="DELETE" @click="remove">删除</SoftButton>
    </div>

    <div class="body">
      <div class="list">
        <input v-model="keyword" type="search" placeholder="搜索备忘…" />
        <div class="rows">
          <button
            v-for="row in visible"
            :key="row.id"
            class="row"
            :class="{ on: current?.id === row.id }"
            @click="select(row)"
          >
            <span class="row-title">{{ row.title || "无标题备忘" }}</span>
            <span class="row-meta">
              {{ row.updated_at.slice(0, 16) }} · {{ snippet(row.html) }}
            </span>
          </button>
          <div v-if="!visible.length" class="empty">还没有备忘：点右上角「新建备忘」</div>
        </div>
      </div>

      <div class="editor-pane">
        <input v-model="title" class="title-input" placeholder="名称（留空就用正文第一行）" />
        <div class="toolbar">
          <select class="block" @change="formatBlock(($event.target as HTMLSelectElement).value)">
            <option value="div">正文</option>
            <option value="h2">标题</option>
            <option value="h3">小标题</option>
            <option value="blockquote">引用</option>
          </select>
          <button @click="exec('bold')"><b>B</b></button>
          <button @click="exec('italic')"><i>I</i></button>
          <button @click="exec('underline')"><u>U</u></button>
          <select class="size" @change="exec('fontSize', ($event.target as HTMLSelectElement).value)">
            <option value="3">16 p</option>
            <option value="2">13 p</option>
            <option value="5">22 p</option>
          </select>
          <label class="color" title="文字颜色">
            <IconGlyph name="EDIT" :size="12" />
            <input type="color" @input="exec('foreColor', ($event.target as HTMLInputElement).value)" />
          </label>
          <button @click="exec('insertUnorderedList')" title="无序列表">• 列表</button>
          <button @click="exec('insertOrderedList')" title="有序列表">1. 列表</button>
          <button @click="exec('outdent')" title="减少缩进">⇤</button>
          <button class="todo" @click="exec('insertHTML', '<div>☐ 待办</div>')" title="插入待办">
            ☐ 待办
          </button>
          <span class="spacer" />
          <span class="saved">{{ status }}</span>
        </div>
        <div
          ref="editor"
          class="editor"
          contenteditable="true"
          spellcheck="false"
          @input="markDirty"
        />
        <div class="foot">{{ words }} 字{{ dirty ? "（未保存）" : "" }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
}
.body {
  flex: 1;
  display: flex;
  gap: 14px;
  min-height: 0;
}
.list {
  width: 306px;
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
}
.rows {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.row {
  display: flex;
  flex-direction: column;
  /* 全局 .row 会给 align-items:center，竖排时会把标题居中，这里显式覆盖成左对齐 */
  align-items: stretch;
  gap: 3px;
  padding: 9px 11px;
  border-radius: var(--radius-card);
  border: 1px solid var(--border-soft);
  background: var(--card);
  text-align: left;
  font-family: inherit;
  cursor: pointer;
}
.row.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 7%, var(--card));
}
.row-title {
  font-size: 13.5px;
  font-weight: 600;
  color: var(--text);
}
.row-meta {
  font-size: 11.5px;
  color: var(--muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.empty {
  font-size: 12.5px;
  color: var(--muted);
  padding: 6px 4px;
}
.editor-pane {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-card);
  padding: 14px 16px;
}
.title-input {
  border: none;
  background: transparent;
  font-size: 16px;
  font-weight: 640;
  padding: 2px 0 6px;
  border-bottom: 1px solid var(--border-soft);
  border-radius: 0;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-wrap: wrap;
  padding: 4px 0 6px;
  border-bottom: 1px solid var(--border-soft);
}
.toolbar button,
.toolbar select,
.toolbar .color {
  height: 26px;
  min-width: 26px;
  padding: 0 7px;
  border-radius: 7px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-soft);
  font-family: inherit;
  font-size: 12px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 3px;
}
.toolbar button:hover,
.toolbar select:hover,
.toolbar .color:hover {
  background: var(--card-alt);
}
.toolbar .block,
.toolbar .size {
  border-color: var(--border-soft);
}
.toolbar .color input {
  width: 16px;
  height: 16px;
  border: none;
  background: transparent;
  padding: 0;
  cursor: pointer;
}
.toolbar .spacer {
  flex: 1;
}
.saved {
  font-size: 11.5px;
  color: var(--muted);
}
.editor {
  flex: 1;
  overflow-y: auto;
  outline: none;
  font-size: 14.5px;
  line-height: 1.75;
  color: var(--text);
  user-select: text;
  padding-top: 6px;
}
.foot {
  font-size: 11.5px;
  color: var(--muted);
  padding-top: 4px;
}
/* 列表行一律左对齐（button 默认居中） */
.rows .row,
.rows .row > span {
  text-align: left;
}
</style>
