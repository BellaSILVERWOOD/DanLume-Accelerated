<script setup lang="ts">
// 通用确认框（用户 2026-11-11：删日程要弹「确认删除吗？删除后无法恢复」）
//
// 用法：
//   <ConfirmDialog v-model:open="open" title="删除这条日程？" message="删除后无法恢复。"
//                  confirm-text="确认删除" @confirm="doDelete" />
import { onBeforeUnmount, watch } from "vue";
import SoftButton from "./SoftButton.vue";

const props = withDefaults(
  defineProps<{
    open: boolean;
    title?: string;
    message?: string;
    confirmText?: string;
    cancelText?: string;
    /** 危险操作：确认键用红色 */
    danger?: boolean;
  }>(),
  {
    title: "确定要删除吗？",
    message: "删除后无法恢复。",
    confirmText: "确认删除",
    cancelText: "取消",
    danger: true,
  },
);

const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
  (e: "confirm"): void;
}>();

function close() {
  emit("update:open", false);
}

function confirm() {
  emit("update:open", false);
  emit("confirm");
}

/** Esc 取消（和别的弹窗一致） */
function onKey(event: KeyboardEvent) {
  if (!props.open) return;
  if (event.key === "Escape") {
    event.preventDefault();
    close();
  } else if (event.key === "Enter") {
    event.preventDefault();
    confirm();
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) window.addEventListener("keydown", onKey);
    else window.removeEventListener("keydown", onKey);
  },
);

onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div v-if="open" class="confirm-mask" @click.self="close">
    <div class="confirm-card">
      <div class="confirm-title">{{ title }}</div>
      <div class="confirm-body">{{ message }}</div>
      <div class="confirm-actions">
        <SoftButton :height="32" :font-size="12.5" @click="close">{{ cancelText }}</SoftButton>
        <SoftButton
          :height="32"
          :font-size="12.5"
          :variant="danger ? undefined : 'primary'"
          :class="{ danger }"
          @click="confirm"
        >
          {{ confirmText }}
        </SoftButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.confirm-mask {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.32);
}
.confirm-card {
  width: min(380px, 90vw);
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 18px 18px 14px;
  border-radius: 14px;
  border: 1px solid var(--border);
  background: var(--card);
  box-shadow: 0 22px 60px rgba(0, 0, 0, 0.24);
}
.confirm-title {
  font-size: 15px;
  font-weight: 650;
  color: var(--text);
}
.confirm-body {
  font-size: 12.5px;
  line-height: 1.7;
  color: var(--muted);
}
.confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding-top: 4px;
}
/* 危险操作的确认键：红字红边，不做光泽（跟全局按钮风格一致） */
.confirm-actions .danger {
  color: var(--danger);
  border-color: var(--danger);
  background: color-mix(in srgb, var(--danger) 10%, var(--card));
}
</style>
