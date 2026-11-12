<script setup lang="ts">
// 通讯录（学生工作）：姓名 / 职位 / 邮箱 / 手机号 / 微信备注名 / QQ / 其它联系方式
//
// 也放在「学生工作」页和设置页里复用；AI 能不能读它由 `contacts_ai_allowed` 控制（默认关）。
import { computed, onMounted, ref } from "vue";
import SoftButton from "./SoftButton.vue";
import ConfirmDialog from "./ConfirmDialog.vue";
import { contactsDelete, contactsList, contactsSave, type Contact } from "../api";

const props = withDefaults(defineProps<{ compact?: boolean }>(), { compact: false });

const rows = ref<Contact[]>([]);
const keyword = ref("");
const busy = ref(false);
const message = ref("");
const editing = ref<Contact | null>(null);
const form = ref({
  id: 0,
  name: "",
  title: "",
  email: "",
  phone: "",
  wechat: "",
  qq: "",
  extra: "",
  note: "",
});
const confirmOpen = ref(false);
const pending = ref<Contact | null>(null);

const visible = computed(() => rows.value);

async function load() {
  busy.value = true;
  try {
    rows.value = await contactsList(keyword.value.trim());
  } catch (err) {
    message.value = `读通讯录失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

function startAdd() {
  editing.value = { id: 0 } as Contact;
  form.value = {
    id: 0,
    name: "",
    title: "",
    email: "",
    phone: "",
    wechat: "",
    qq: "",
    extra: "",
    note: "",
  };
}

function startEdit(row: Contact) {
  editing.value = row;
  form.value = {
    id: row.id,
    name: row.name,
    title: row.title || "",
    email: row.email || "",
    phone: row.phone || "",
    wechat: row.wechat || "",
    qq: row.qq || "",
    extra: row.extra || "",
    note: row.note || "",
  };
}

async function submit() {
  if (!form.value.name.trim()) {
    message.value = "姓名不能为空";
    return;
  }
  busy.value = true;
  try {
    await contactsSave({ ...form.value });
    editing.value = null;
    message.value = "已保存";
    await load();
  } catch (err) {
    message.value = `保存失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

function askRemove(row: Contact) {
  pending.value = row;
  confirmOpen.value = true;
}

async function doRemove() {
  const row = pending.value;
  pending.value = null;
  if (!row) return;
  try {
    await contactsDelete(row.id);
    message.value = `已删掉 ${row.name}`;
    await load();
  } catch (err) {
    message.value = `删除失败：${String(err)}`;
  }
}

onMounted(load);
</script>

<template>
  <div class="contacts" :class="{ compact }">
    <div class="bar">
      <input
        v-model="keyword"
        type="text"
        class="search"
        placeholder="搜姓名 / 职位 / 邮箱 / 微信 / 手机号"
        spellcheck="false"
        @keyup.enter="load"
      />
      <SoftButton :height="28" :font-size="12" :disabled="busy" @click="load">搜索</SoftButton>
      <SoftButton variant="primary" :height="28" :font-size="12" @click="startAdd">＋ 加联系人</SoftButton>
      <span class="spacer" />
      <span class="tiny">{{ rows.length }} 人</span>
    </div>

    <div v-if="message" class="hint">{{ message }}</div>

    <div v-if="editing" class="editor">
      <div class="editor-head">
        <b>{{ form.id ? "改联系人" : "新联系人" }}</b>
        <span class="spacer" />
        <SoftButton :height="26" :font-size="11.5" @click="editing = null">取消</SoftButton>
        <SoftButton variant="primary" :height="26" :font-size="11.5" :disabled="busy" @click="submit">
          保存
        </SoftButton>
      </div>
      <div class="grid">
        <label><span>姓名 *</span><input v-model="form.name" type="text" spellcheck="false" /></label>
        <label><span>职位</span><input v-model="form.title" type="text" placeholder="如：辅导员 / 班长" /></label>
        <label><span>邮箱</span><input v-model="form.email" type="text" spellcheck="false" /></label>
        <label><span>手机号</span><input v-model="form.phone" type="text" spellcheck="false" /></label>
        <label>
          <span>微信备注名</span>
          <input v-model="form.wechat" type="text" placeholder="你在微信里给 TA 的备注" />
        </label>
        <label><span>QQ</span><input v-model="form.qq" type="text" spellcheck="false" /></label>
        <label class="wide">
          <span>其它联系方式</span>
          <input v-model="form.extra" type="text" placeholder="飞书 / 钉钉 / 企业微信 / webhook 地址…" />
        </label>
        <label class="wide"><span>备注</span><input v-model="form.note" type="text" /></label>
      </div>
    </div>

    <div class="list">
      <div v-if="!visible.length" class="empty">
        还没有联系人。点「＋ 加联系人」把同学 / 老师的联系方式记进来。
      </div>
      <div v-for="row in visible" :key="row.id" class="row-item">
        <div class="who">
          <b>{{ row.name }}</b>
          <span v-if="row.title" class="title-tag">{{ row.title }}</span>
        </div>
        <div class="fields">
          <span v-if="row.email">✉ {{ row.email }}</span>
          <span v-if="row.phone">☎ {{ row.phone }}</span>
          <span v-if="row.wechat">微信：{{ row.wechat }}</span>
          <span v-if="row.qq">QQ：{{ row.qq }}</span>
          <span v-if="row.extra">其它：{{ row.extra }}</span>
        </div>
        <span class="spacer" />
        <SoftButton :height="24" :font-size="11" @click="startEdit(row)">改</SoftButton>
        <SoftButton :height="24" :font-size="11" @click="askRemove(row)">删</SoftButton>
      </div>
    </div>

    <ConfirmDialog
      v-model:open="confirmOpen"
      title="确定删掉这个联系人吗？"
      :message="`「${pending?.name || ''}」删除后无法恢复（已经发出去的定时通知不受影响）。`"
      confirm-text="确认删除"
      @confirm="doRemove"
    />
  </div>
</template>

<style scoped>
.contacts {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.bar {
  display: flex;
  align-items: center;
  gap: 6px;
}
.search {
  flex: 1;
  min-width: 120px;
}
.editor {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
}
.editor-head {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
}
.grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px 10px;
}
.grid label {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.grid label.wide {
  grid-column: span 2;
}
.grid label > span {
  font-size: 11px;
  color: var(--muted);
}
.grid input {
  height: 28px;
  font-size: 12px;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 320px;
  overflow-y: auto;
}
.row-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 9px;
  border-radius: 9px;
  border: 1px solid var(--border-soft);
  background: var(--card);
}
.who {
  display: flex;
  align-items: center;
  gap: 5px;
  flex: none;
}
.who b {
  font-size: 12.5px;
}
.title-tag {
  font-size: 10.5px;
  color: var(--muted);
  border: 1px solid var(--border-soft);
  border-radius: 999px;
  padding: 0 5px;
}
.fields {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
  font-size: 11.5px;
  color: var(--muted);
  overflow: hidden;
}
.empty {
  font-size: 12px;
  color: var(--muted);
  padding: 8px 2px;
}
.tiny {
  font-size: 11px;
  color: var(--muted);
}
.hint {
  font-size: 11.5px;
  color: var(--muted);
}
.spacer {
  flex: 1;
}
.compact .list {
  max-height: 220px;
}
</style>
