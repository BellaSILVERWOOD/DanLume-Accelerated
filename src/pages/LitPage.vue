<script setup lang="ts">
// 文献库：读旧库 papers 表，项目 / 星级 / 搜索筛选，右侧详情 + 引用
import { computed, onMounted, ref } from "vue";
import SoftButton from "../components/SoftButton.vue";
import Pill from "../components/Pill.vue";
import IconBadge from "../components/IconBadge.vue";
import {
  openPath,
  paperCitation,
  paperUpdate,
  papersList,
  type Paper,
} from "../api";

const items = ref<Paper[]>([]);
const projects = ref<{ name: string; total: number }[]>([]);
const project = ref("");
const starsMin = ref(0);
const keyword = ref("");
const selectedId = ref<number | null>(null);
const message = ref("");
const selected = computed(
  () => items.value.find((item) => item.id === selectedId.value) || items.value[0] || null,
);

async function load() {
  const result = await papersList(project.value, starsMin.value, keyword.value);
  items.value = result.items || [];
  projects.value = result.projects || [];
  if (!items.value.find((item) => item.id === selectedId.value)) {
    selectedId.value = items.value.length ? items.value[0].id : null;
  }
  message.value = items.value.length ? "" : "还没有文献（旧版里读过 / 下载过的会出现在这里）";
}

async function toggleStar(paper: Paper, star: number) {
  const next = paper.stars === star ? 0 : star;
  await paperUpdate(paper.id, { stars: next });
  paper.stars = next;
}

async function copyCitation() {
  if (!selected.value) return;
  const result = await paperCitation(selected.value.id);
  if (!result.ok) {
    message.value = "取引用失败";
    return;
  }
  try {
    await navigator.clipboard.writeText(result.text);
    message.value = "已复制引用（GB/T 7714）";
  } catch {
    message.value = result.text;
  }
}

const stars = [1, 2, 3, 4, 5];

onMounted(load);
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">文献库</h1>
        <div class="muted">共 {{ items.length }} 篇 · 项目、星级、引用一条龙</div>
      </div>
      <div class="spacer" />
      <SoftButton :height="32" :font-size="12.5" glyph="REFRESH" @click="load">刷新</SoftButton>
      <SoftButton :height="32" :font-size="12.5" glyph="FOLDER" @click="openPath('')">
        打开文献库目录
      </SoftButton>
    </div>

    <div class="bar">
      <input v-model="keyword" type="search" placeholder="过滤本机文献：标题 / 作者 / 期刊…" @change="load" />
      <div class="projects">
        <button :class="{ on: !project }" @click="(project = ''), load()">全部</button>
        <button
          v-for="one in projects"
          :key="one.name"
          :class="{ on: project === one.name }"
          @click="(project = one.name), load()"
        >
          {{ one.name || "未分类" }} ({{ one.total }})
        </button>
      </div>
      <select v-model.number="starsMin" @change="load">
        <option :value="0">全部星级</option>
        <option :value="1">1 星以上</option>
        <option :value="3">3 星以上</option>
        <option :value="5">只看 5 星</option>
      </select>
    </div>

    <div class="body">
      <div class="list">
        <button
          v-for="paper in items"
          :key="paper.id"
          class="row"
          :class="{ on: selected?.id === paper.id }"
          @click="selectedId = paper.id"
        >
          <span class="stars">
            <b v-for="star in stars" :key="star" :class="{ lit: paper.stars >= star }" @click.stop="toggleStar(paper, star)">★</b>
          </span>
          <span class="text">
            <span class="title">{{ paper.title }}</span>
            <span class="meta">
              {{ paper.authors.slice(0, 40) }} · {{ paper.venue }} · {{ paper.year || "" }}
            </span>
            <span v-if="paper.summary_zh" class="summary">{{ paper.summary_zh }}</span>
          </span>
          <span class="project">{{ paper.project || "未分类" }}</span>
        </button>
        <div v-if="!items.length" class="empty">{{ message }}</div>
      </div>

      <div class="detail">
        <template v-if="selected">
          <div class="detail-head">
            <IconBadge glyph="DOCUMENT" :tone="selected.stars >= 3 ? 'rose' : 'sky'" :size="30" />
            <div class="head-text">
              <div class="title">{{ selected.title }}</div>
              <div class="meta">{{ selected.authors }}</div>
              <div class="meta">{{ selected.venue }} · {{ selected.year }} · {{ selected.doi }}</div>
            </div>
          </div>
          <div class="scroll">
            <div class="field">
              <div class="label">摘要</div>
              <div class="text">{{ selected.abstract || "（没有摘要）" }}</div>
            </div>
            <div class="field">
              <div class="label">中文概述</div>
              <div class="text">{{ selected.summary_zh || "（还没有自动概述）" }}</div>
            </div>
            <div class="field">
              <div class="label">备注</div>
              <div class="text">{{ selected.note || "（空）" }}</div>
            </div>
          </div>
          <div class="detail-foot">
            <Pill :text="`${selected.stars} 星`" :font-size="11.5" />
            <SoftButton :height="28" :font-size="12" @click="copyCitation">复制引用</SoftButton>
            <SoftButton
              :height="28"
              :font-size="12"
              :disabled="!selected.file_path"
              @click="openPath(selected.file_path)"
            >
              打开 PDF
            </SoftButton>
          </div>
        </template>
        <div v-else class="empty">选左边一篇文献</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 100%;
}
.bar {
  display: flex;
  align-items: center;
  gap: 10px;
}
.bar input {
  width: 300px;
}
.projects {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}
.projects button {
  height: 24px;
  padding: 0 10px;
  border-radius: var(--radius-pill);
  border: 1px solid var(--border-soft);
  background: var(--card);
  color: var(--muted);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.projects button.on {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 8%, var(--card));
}
.body {
  flex: 1;
  display: flex;
  gap: 12px;
  min-height: 0;
}
.list {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.row {
  display: flex;
  align-items: flex-start;
  gap: 9px;
  padding: 9px 11px;
  border-radius: var(--radius-card);
  border: 1px solid var(--border-soft);
  background: var(--card);
  font-family: inherit;
  text-align: left;
  cursor: pointer;
}
.row.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 6%, var(--card));
}
.stars {
  display: flex;
  gap: 1px;
  font-size: 11px;
  color: var(--border);
  flex: none;
}
.stars b {
  font-weight: 400;
}
.stars b.lit {
  color: #f5a623;
}
.text {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.title {
  font-size: 13.5px;
  color: var(--text);
  overflow-wrap: anywhere;
}
.meta {
  font-size: 11.5px;
  color: var(--muted);
}
.summary {
  font-size: 11.5px;
  color: var(--text-soft);
  overflow: hidden;
  text-overflow: ellipsis;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}
.project {
  font-size: 11px;
  color: var(--faint);
  white-space: nowrap;
}
.detail {
  width: 42%;
  min-width: 340px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-card);
  padding: 12px 14px;
  min-height: 0;
}
.detail-head {
  display: flex;
  gap: 10px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-soft);
}
.head-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.head-text .title {
  font-size: 14.5px;
  font-weight: 640;
}
.scroll {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.label {
  font-size: 11.5px;
  color: var(--faint);
}
.text {
  font-size: 12.5px;
  line-height: 1.7;
  color: var(--text-soft);
  user-select: text;
}
.detail-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 8px;
  border-top: 1px solid var(--border-soft);
}
.empty {
  font-size: 12.5px;
  color: var(--muted);
  padding: 8px 2px;
}
/* 列表行一律左对齐（button 默认居中） */
.list .row,
.list .row > span {
  text-align: left;
}
</style>
