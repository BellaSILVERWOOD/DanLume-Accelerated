<script setup lang="ts">
// AI 面板里的「提示词」抽屉（对应旧版 ai_prompt_panel.py + prompt_gen.py）。
//
// 两个页签：
// * 模板：内置系统提示词（只能改）+ 场景模板（可增删），点「用」把内容填进输入框；
// * 生成：写一句需求 → 让当前 AI 产出一份结构化提示词 → 预览 → 保存成模板或直接用。
// 数据全在 Rust 侧（prompts.rs，存 settings 的 prompts_json，结构与旧版 prompts.json 一致）。
import { onMounted, ref } from "vue";
import IconGlyph from "./IconGlyph.vue";
import SoftButton from "./SoftButton.vue";
import {
  promptGenerate,
  promptTemplateDelete,
  promptTemplateSave,
  promptTemplates,
  type PromptTemplate,
} from "../api";

const emit = defineEmits<{ (e: "close"): void; (e: "use", text: string): void }>();

const tab = ref<"templates" | "generate">("templates");
const items = ref<PromptTemplate[]>([]);
const current = ref<PromptTemplate | null>(null);
const isNew = ref(false);
const editText = ref("");
const editTitle = ref("");
const busy = ref(false);
const error = ref("");
const message = ref("");

// 生成器
const requirement = ref("");
const generated = ref("");
const genHint = ref("");
const genError = ref("");

async function refresh() {
  error.value = "";
  try {
    const result = await promptTemplates();
    if (!result.ok) {
      error.value = result.error || "读不到提示词";
      return;
    }
    items.value = result.items;
  } catch (err) {
    error.value = `读取提示词失败：${String(err)}`;
  }
}

function select(item: PromptTemplate) {
  current.value = item;
  isNew.value = false;
  editTitle.value = item.title;
  editText.value = item.text;
  error.value = "";
  message.value = "";
}

function startNew() {
  current.value = null;
  isNew.value = true;
  editTitle.value = "";
  editText.value = "";
  error.value = "";
  message.value = "";
}

/** 把模板内容交给对话面板（父组件负责填进输入框） */
function useText(text: string) {
  const trimmed = (text || "").trim();
  if (!trimmed) {
    error.value = "这条是空的，先写点内容";
    return;
  }
  emit("use", trimmed);
  emit("close");
}

/** 模板标识：名字去掉空格和路径里那些符号（跟 Rust 侧 validate_key 一致） */
function keyOf(title: string): string {
  return title.trim().replace(/[\s:/\\|?*"<>]+/g, "");
}

async function save() {
  const text = editText.value.trim();
  if (!text) {
    error.value = "提示词内容不能为空";
    return;
  }
  const kind = isNew.value ? "scene" : current.value?.kind || "scene";
  const title = editTitle.value.trim();
  const key = isNew.value ? keyOf(title) : current.value?.key || "";
  if (!key) {
    error.value = "给这条模板起个名字（它会当标识用，不能带空格）";
    return;
  }
  busy.value = true;
  error.value = "";
  try {
    const result = await promptTemplateSave(kind, key, title, text);
    if (!result.ok) {
      error.value = result.error || "保存失败";
      return;
    }
    await refresh();
    const found = items.value.find((one) => one.kind === kind && one.key === (result.key || key));
    if (found) select(found);
    else isNew.value = false;
    message.value = `已保存「${title || key}」`;
  } catch (err) {
    error.value = `保存失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

async function removeTemplate() {
  const item = current.value;
  if (!item) return;
  if (item.kind !== "scene") {
    error.value = "内置系统提示词不能删除（可以改内容）";
    return;
  }
  busy.value = true;
  error.value = "";
  try {
    const result = await promptTemplateDelete(item.kind, item.key);
    if (!result.ok) {
      error.value = result.error || "删不掉";
      return;
    }
    message.value = `已删除「${item.title}」`;
    current.value = null;
    isNew.value = false;
    editTitle.value = "";
    editText.value = "";
    await refresh();
  } catch (err) {
    error.value = `删除失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

async function doGenerate() {
  genError.value = "";
  genHint.value = "";
  generated.value = "";
  const text = requirement.value.trim();
  if (!text) {
    genError.value = "先写一句你要什么（比如「帮我把论文摘要压成三句话」）";
    return;
  }
  busy.value = true;
  try {
    const result = await promptGenerate(text);
    if (!result.ok) {
      genError.value = result.error || "生成失败";
      genHint.value = result.hint || "";
      return;
    }
    generated.value = result.text || "";
    genHint.value = result.model
      ? `由 ${result.model} 生成 · ${result.chars ?? generated.value.length} 字，可以再改`
      : "生成好了，可以再改";
  } catch (err) {
    genError.value = `生成失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

async function saveGenerated() {
  const text = generated.value.trim();
  if (!text) return;
  const title = keyOf(requirement.value).slice(0, 20) || "生成的提示词";
  busy.value = true;
  genError.value = "";
  try {
    const result = await promptTemplateSave("scene", title, title, text);
    if (!result.ok) {
      genError.value = result.error || "保存失败";
      return;
    }
    genHint.value = `已保存为场景模板「${title}」，在「模板」里能改能删`;
    await refresh();
  } catch (err) {
    genError.value = `保存失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

onMounted(refresh);
</script>

<template>
  <div class="drawer">
    <header class="head">
      <div class="tabs">
        <button class="tab" :class="{ on: tab === 'templates' }" @click="tab = 'templates'">
          模板
        </button>
        <button class="tab" :class="{ on: tab === 'generate' }" @click="tab = 'generate'">
          生成
        </button>
      </div>
      <span class="spacer" />
      <button class="icon" title="关闭（回到对话）" @click="emit('close')">
        <IconGlyph name="CLOSE" :size="12" />
      </button>
    </header>

    <div v-if="tab === 'templates'" class="pane">
      <div class="section-title">提示词模板</div>
      <div class="list">
        <button
          v-for="item in items"
          :key="item.kind + ':' + item.key"
          class="row"
          :class="{ on: current && current.kind === item.kind && current.key === item.key }"
          @click="select(item)"
        >
          <span class="kind" :class="item.kind">{{ item.kind === "system" ? "系统" : "场景" }}</span>
          <span class="name">{{ item.title }}</span>
          <span v-if="item.builtin" class="builtin">内置</span>
          <span class="use" title="填进输入框" @click.stop="useText(item.text)">用</span>
        </button>
        <div v-if="!items.length" class="tiny">还没有模板，点下面的「新建」。</div>
      </div>

      <div class="section-title">
        {{ isNew ? "新建场景模板" : current ? `编辑：${current.title}` : "选一条模板" }}
      </div>
      <input
        v-model="editTitle"
        class="title-input"
        type="text"
        :disabled="!isNew && current?.kind === 'system'"
        placeholder="显示名（新建时也会当标识，别带空格）"
      />
      <textarea
        v-model="editText"
        class="text"
        rows="7"
        placeholder="在这里写提示词…"
        :disabled="!isNew && !current"
      />
      <div class="actions">
        <SoftButton :height="26" :font-size="11.5" @click="startNew">新建</SoftButton>
        <SoftButton :height="26" :font-size="11.5" :disabled="busy || (!isNew && !current)" @click="save">
          保存
        </SoftButton>
        <SoftButton
          variant="primary"
          :height="26"
          :font-size="11.5"
          :disabled="!editText.trim()"
          @click="useText(editText)"
        >
          用这条
        </SoftButton>
        <SoftButton
          v-if="!isNew && current && current.kind === 'scene'"
          variant="danger"
          :height="26"
          :font-size="11.5"
          :disabled="busy"
          @click="removeTemplate"
        >
          删除
        </SoftButton>
        <span class="spacer" />
        <span v-if="isNew" class="tiny">取消点左边「模板」里任意一条</span>
      </div>
      <div v-if="message" class="tiny">{{ message }}</div>
      <div v-if="error" class="err">{{ error }}</div>
    </div>

    <div v-else class="pane">
      <div class="section-title">生成提示词</div>
      <div class="tiny">
        写一句你要什么，让当前 AI（设置 → AI）写一份结构化提示词：角色 / 目标 / 输出格式 / 约束。
      </div>
      <textarea
        v-model="requirement"
        class="text"
        rows="3"
        placeholder="比如：帮我把论文摘要压成三句话，语气客观，不要编造数据"
      />
      <div class="actions">
        <SoftButton variant="primary" :height="26" :font-size="11.5" :disabled="busy" @click="doGenerate">
          {{ busy ? "生成中…" : "生成" }}
        </SoftButton>
        <span class="spacer" />
        <span class="tiny">没配 API Key 时会提示去设置里填</span>
      </div>
      <textarea v-if="generated" v-model="generated" class="text" rows="8" />
      <div v-if="generated" class="actions">
        <SoftButton :height="26" :font-size="11.5" :disabled="busy" @click="saveGenerated">
          保存为模板
        </SoftButton>
        <SoftButton variant="primary" :height="26" :font-size="11.5" @click="useText(generated)">
          用这条
        </SoftButton>
      </div>
      <div v-if="genHint" class="tiny">{{ genHint }}</div>
      <div v-if="genError" class="err">{{ genError }}</div>
    </div>
  </div>
</template>

<style scoped>
.drawer {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 5px;
  min-height: 0;
  overflow: hidden;
}
.head {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: none;
}
.tabs {
  display: flex;
  gap: 4px;
}
.tab {
  height: 22px;
  padding: 0 9px;
  border-radius: var(--radius-pill);
  border: none;
  background: transparent;
  color: var(--muted);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.tab.on {
  background: var(--card);
  color: var(--text);
  font-weight: 600;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
}
.icon {
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  color: var(--muted);
  border-radius: 6px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.icon:hover {
  background: var(--card-alt);
}
.spacer {
  flex: 1;
}
.pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 5px;
  min-height: 0;
  overflow-y: auto;
}
.section-title {
  font-size: 12px;
  font-weight: 640;
  color: var(--text-soft);
  margin: 3px 1px 1px;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 3px;
  max-height: 168px;
  overflow-y: auto;
}
.row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  border-radius: 8px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  font-family: inherit;
  font-size: 12px;
  color: var(--text);
  text-align: left;
  cursor: pointer;
}
.row.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 8%, var(--card));
}
.row .name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.kind {
  flex: none;
  font-size: 10px;
  padding: 1px 5px;
  border-radius: 999px;
  background: var(--card-alt);
  color: var(--muted);
}
.kind.system {
  background: var(--accent-soft, color-mix(in srgb, var(--accent) 12%, transparent));
  color: var(--accent);
}
.builtin {
  flex: none;
  font-size: 10px;
  color: var(--faint);
}
.use {
  flex: none;
  font-size: 11px;
  color: var(--accent);
  padding: 0 2px;
}
.use:hover {
  text-decoration: underline;
}
.title-input {
  height: 26px;
  border-radius: 7px;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  color: var(--text);
  font-family: inherit;
  font-size: 12px;
  padding: 0 8px;
}
.text {
  width: 100%;
  resize: vertical;
  border-radius: 9px;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  color: var(--text);
  font-family: inherit;
  font-size: 12.5px;
  line-height: 1.6;
  padding: 6px 8px;
  min-height: 60px;
}
.actions {
  display: flex;
  align-items: center;
  gap: 5px;
  flex-wrap: wrap;
}
.tiny {
  font-size: 11px;
  color: var(--muted);
  line-height: 1.5;
}
.err {
  font-size: 11.5px;
  color: var(--danger);
  line-height: 1.5;
  overflow-wrap: anywhere;
}
</style>
