//! 统一身份认证的「网页自动填表」（与 Python 版 `uis_login.autofill_script` 同一套做法）。
//!
//! 复旦已经把 UIS 换成 `id.fudan.edu.cn/ac/`（Vue 单页 + RSA 加密接口），后台拼表单登录
//! 那条路下线了。Python 版的做法是：**把学号密码注入到我们自己的内嵌网页登录页里，由页面
//! 自己加密提交**，登录成功后再从网页里取会话。Rust 版这里保持一致：
//!
//! * 只做「填表 + 点登录」，不逆向任何加密；
//! * 需要二次验证（短信 / 邮箱 / 扫码）时用户手动过一次；
//! * 密码只在本机 DPAPI 里，注入时做 JSON 转义；
//! * 新版是 SPA：`load` 之后还会重渲染，早填的值会被框架抹掉，所以脚本里**带重试**。
//!
//! 注意：这段 JS 由 `open_uis_window` 用 `initialization_script` 注入到 `uis-login` 窗口，
//! 每次页面导航都会重新执行一次。

use crate::store::Store;

/// 与 Python 版字段键一致
pub const USER_KEY: &str = "uis_username";
pub const PASS_KEY: &str = "uis_password";
pub const AUTOFILL_KEY: &str = "uis_autofill";

/// 登录页所在的主机（这两台上都算「登录页」，不在上面时才认为登录成功了）
pub fn is_login_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "id.fudan.edu.cn"
        || host.ends_with(".id.fudan.edu.cn")
        || host == "uis.fudan.edu.cn"
        || host.ends_with(".uis.fudan.edu.cn")
}

/// 能不能自动填（Python 版 `autofill_enabled` + `has_credentials`）
pub fn autofill_enabled(store: &Store) -> bool {
    store
        .setting(AUTOFILL_KEY)
        .map(|text| text.trim() != "0")
        .unwrap_or(true)
}

pub fn credentials(store: &Store) -> (String, String) {
    let user = store.setting(USER_KEY).unwrap_or_default().trim().to_string();
    let pass = store
        .setting(PASS_KEY)
        .filter(|text| !text.trim().is_empty())
        .and_then(|text| crate::secrets::unprotect(&text).ok())
        .unwrap_or_default();
    (user, pass)
}

/// 生成注入脚本；没有账号/密码或关掉自动填表时返回空串（调用方据此跳过注入）。
///
/// 两段：① 登录页里填表 + 点登录（`AUTOFILL_JS`）；② 还没登录的业务页里点一下「登录」
/// 入口（`AUTO_ENTER_JS`）——用户要求「所有内嵌网页都要有自动 UIS 登录」，
/// 光有①不够：eLearning / eHall 首页是无密码框的欢迎页，得先点「登录」才会跳到 UIS。
pub fn script(store: &Store) -> String {
    let (user, pass) = credentials(store);
    if user.is_empty() || pass.is_empty() || !autofill_enabled(store) {
        return String::new();
    }
    let user_json = serde_json::to_string(&user).unwrap_or_else(|_| "\"\"".to_string());
    let pass_json = serde_json::to_string(&pass).unwrap_or_else(|_| "\"\"".to_string());
    let fill = AUTOFILL_JS
        .replace("__USER__", &user_json)
        .replace("__PWD__", &pass_json);
    format!("{fill}\n{AUTO_ENTER_JS}")
}

/// 业务页上的「登录」入口：没有密码框时点一下（每个站点在一个会话里只点一次，
/// 免得用户在登录页上手动操作时被反复弹走）。
const AUTO_ENTER_JS: &str = r#"
(function(){
  var KEY = 'danlume-autologin';
  function norm(text){ return String(text || '').replace(/\s+/g, '').toLowerCase(); }
  function shown(el){
    try {
      var s = getComputedStyle(el);
      if (s.display === 'none' || s.visibility === 'hidden' || s.opacity === '0') { return false; }
      var r = el.getBoundingClientRect();
      return r.width >= 12 && r.height >= 8;
    } catch (e) { return false; }
  }
  function attempt(){
    try {
      if (document.querySelector('input[type="password"]')) { return; }
      if (sessionStorage.getItem(KEY) === location.host) { return; }
      var nodes = document.querySelectorAll('a, button, [role="button"], span, li, div');
      for (var i = 0; i < nodes.length; i++) {
        var el = nodes[i];
        if (el.children && el.children.length > 0) { continue; }
        var text = norm(el.innerText || el.textContent);
        if (text !== '登录' && text !== '登陆' && text !== 'login' && text !== 'signin') { continue; }
        if (!shown(el)) { continue; }
        sessionStorage.setItem(KEY, location.host);
        try { el.click(); } catch (e) {}
        try { console.log('[DanLume autologin] 点「登录」入口：', location.href); } catch (e) {}
        return;
      }
    } catch (e) {}
  }
  setTimeout(attempt, 2200);
  setTimeout(attempt, 7000);
})();
"#;

/// 移植自 Python `uis_login.AUTOFILL_JS`：
/// ① 连 iframe 一起找；② 用原生 setter + input/change 事件让框架认；
/// ③ 填完校验真的写进去了；④ 只有校验通过才点「登录」；⑤ 整体重试多次。
const AUTOFILL_JS: &str = r#"
(function(){
  var user = __USER__, pwd = __PWD__;
  if (!user || !pwd) { return; }
  var tries = 0, maxTries = 9;

  function docs(){
    var out = [document];
    try {
      var frames = document.querySelectorAll('iframe, frame');
      for (var i = 0; i < frames.length; i++){
        try { var d = frames[i].contentDocument; if (d) { out.push(d); } } catch (e) {}
      }
    } catch (e) {}
    return out;
  }
  function nativeSet(el, value){
    try {
      var desc = Object.getOwnPropertyDescriptor(el.constructor.prototype, 'value');
      if (desc && desc.set) { desc.set.call(el, value); } else { el.value = value; }
    } catch (e) { try { el.value = value; } catch (e2) { return false; } }
    try {
      el.dispatchEvent(new InputEvent('input', {bubbles: true, data: value, inputType: 'insertText'}));
    } catch (e) { el.dispatchEvent(new Event('input', {bubbles: true})); }
    el.dispatchEvent(new Event('change', {bubbles: true}));
    el.dispatchEvent(new KeyboardEvent('keyup', {bubbles: true, key: 'a'}));
    try { el.dispatchEvent(new Event('blur', {bubbles: true})); } catch (e) {}
    return true;
  }
  function visible(el){
    try {
      var r = el.getBoundingClientRect();
      if (r.width < 16 || r.height < 8) { return false; }
      var s = getComputedStyle(el);
      return s.display !== 'none' && s.visibility !== 'hidden' && s.opacity !== '0';
    } catch (e) { return true; }
  }
  function editable(el){
    if (!el || el.readOnly || el.disabled) { return false; }
    var type = (el.type || '').toLowerCase();
    if (['hidden','submit','button','checkbox','radio','file'].indexOf(type) >= 0) { return false; }
    return visible(el);
  }
  function blob(el){
    var parts = [el.getAttribute('name'), el.id, el.getAttribute('placeholder'),
                 el.getAttribute('aria-label'), el.getAttribute('autocomplete'),
                 el.getAttribute('title'),
                 (el.form && (el.form.id || el.form.className)) || ''];
    return parts.join(' ').toLowerCase();
  }
  function score(el){
    var b = blob(el), s = 0;
    if (/user|account|login|学号|工号|账号|用户|uname/.test(b)) { s += 6; }
    if (el.getAttribute('autocomplete') === 'username') { s += 6; }
    if (/lang|language|locale|语言/i.test(b)) { s -= 12; }
    if (/search|搜索|query|filter/i.test(b)) { s -= 8; }
    if ((el.type || '').toLowerCase() === 'search') { s -= 4; }
    return s;
  }
  function before(a, b){
    try { return !!(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING); }
    catch (e) { return true; }
  }
  function norm(text){ return String(text || '').replace(/\s+/g, '').toLowerCase(); }
  function shown(el){
    try {
      var s = getComputedStyle(el);
      return s.display !== 'none' && s.visibility !== 'hidden' && s.opacity !== '0';
    } catch (e) { return true; }
  }
  function looksLogin(node){
    var t = norm(node.innerText || node.value || node.textContent);
    if (!t || t.length > 12) { return false; }
    return /^(登录|登陆|login|signin|submit|提交|确定|下一步|next)$/.test(t);
  }
  function pressEnter(el){
    var opts = {bubbles: true, cancelable: true, key: 'Enter', code: 'Enter', keyCode: 13, which: 13};
    try { el.dispatchEvent(new KeyboardEvent('keydown', opts)); } catch (e) {}
    try { el.dispatchEvent(new KeyboardEvent('keypress', opts)); } catch (e) {}
    try { el.dispatchEvent(new KeyboardEvent('keyup', opts)); } catch (e) {}
  }
  function clickNodes(nodes, exact){
    for (var i = 0; i < nodes.length; i++){
      var node = nodes[i];
      try {
        if (!node || node.disabled) { continue; }
        if (!shown(node)) { continue; }
        var text = norm(node.innerText || node.value || node.textContent);
        if (exact) {
          if (text !== '登录' && text !== '登陆' && text !== 'login' && text !== 'signin') { continue; }
        } else if (!looksLogin(node)) { continue; }
        node.click();
        return true;
      } catch (e) {}
    }
    return false;
  }
  function clickLogin(found){
    var real = 'button, a, input[type="submit"], input[type="button"], '
      + 'input[type="image"], div[role="button"], [role="button"]';
    var nodes = [];
    try { nodes = Array.prototype.slice.call(found.doc.querySelectorAll(real)); } catch (e) { nodes = []; }
    if (clickNodes(nodes, false)) { return true; }
    var byClass = '';
    try {
      byClass = '[class*="login"], [class*="Login"], [class*="submit"], '
        + '[class*="btn"], [id*="login"], [id*="Login"], [id*="submit"]';
      nodes = Array.prototype.slice.call(found.doc.querySelectorAll(byClass));
    } catch (e) { nodes = []; }
    if (clickNodes(nodes, false)) { return true; }
    var all = [];
    try { all = Array.prototype.slice.call(found.doc.querySelectorAll('*')); } catch (e) { all = []; }
    if (clickNodes(all, true)) { return true; }
    try { found.p.focus(); } catch (e) {}
    pressEnter(found.p);
    pressEnter(found.u);
    try {
      var form = found.p.form;
      if (form && form.requestSubmit) { form.requestSubmit(); return true; }
      if (form) { form.submit(); return true; }
    } catch (e) {}
    return false;
  }
  function findFields(doc){
    var all = [];
    try { all = Array.prototype.slice.call(doc.querySelectorAll('input')); } catch (e) { return null; }
    var passwords = all.filter(function(el){
      return (el.type || '').toLowerCase() === 'password' && editable(el);
    });
    if (!passwords.length) {
      passwords = all.filter(function(el){
        return editable(el) && /pwd|password|密码/.test(blob(el));
      });
    }
    for (var k = 0; k < passwords.length; k++) {
      var p = passwords[k];
      var scope = p.form;
      if (!scope) {
        var node = p.parentElement;
        for (var d = 0; d < 6 && node; d++) {
          try {
            if (node.querySelectorAll && node.querySelectorAll('input').length > 1) { scope = node; break; }
          } catch (e) {}
          node = node.parentElement;
        }
      }
      if (!scope) { scope = doc; }
      var pool = all;
      try {
        var scoped = Array.prototype.slice.call(scope.querySelectorAll('input'));
        if (scoped.length > 1) { pool = scoped; }
      } catch (e) {}
      var cands = [];
      for (var m = 0; m < pool.length; m++) {
        var el2 = pool[m];
        if (el2 === p || !editable(el2)) { continue; }
        var type2 = (el2.type || '').toLowerCase();
        if (type2 && ['text','tel','email','number','search'].indexOf(type2) < 0) { continue; }
        if (!before(el2, p)) { continue; }
        cands.push(el2);
      }
      if (!cands.length) { continue; }
      var best = null, bestScore = -1e9;
      for (var n = 0; n < cands.length; n++) {
        var sc = score(cands[n]);
        if (sc > bestScore) { bestScore = sc; best = cands[n]; }
      }
      if (bestScore <= 0) { best = cands[cands.length - 1]; }
      if (best) { return {doc: doc, u: best, p: p, score: bestScore}; }
    }
    return null;
  }
  function fillOnce(){
    var found = null;
    var all = docs();
    for (var d = 0; d < all.length && !found; d++){ found = findFields(all[d]); }
    if (!found) { return {ok: false, reason: '没找到账号密码输入框', url: location.href}; }
    try { found.u.focus(); } catch (e) {}
    nativeSet(found.u, user);
    try { found.p.focus(); } catch (e) {}
    nativeSet(found.p, pwd);
    var userOk = (found.u.value === user);
    var pwdOk = (found.p.value === pwd);
    if (!userOk || !pwdOk){
      return {ok: false, reason: '登录框没接受输入（页面可能刚刷新，正在重试）',
              userOk: userOk, pwdOk: pwdOk, url: location.href};
    }
    var clicked = false;
    try { clicked = clickLogin(found); } catch (e) {}
    return {ok: true, clicked: clicked, url: location.href};
  }
  function attempt(){
    tries++;
    var result = fillOnce();
    try { console.log('[DanLume autofill]', tries, JSON.stringify(result)); } catch (e) {}
    if (result && result.ok && result.clicked) { return; }
    if (tries < maxTries) { setTimeout(attempt, 350 * tries); }
  }
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', attempt);
  } else {
    attempt();
  }
  setTimeout(attempt, 600);
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard(Option<std::ffi::OsString>);
    impl EnvGuard {
        fn new(name: &str) -> Self {
            let guard = EnvGuard(std::env::var_os("LUMINA_DATA_DIR"));
            let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("LUMINA_DATA_DIR", &dir);
            guard
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("LUMINA_DATA_DIR", value),
                None => std::env::remove_var("LUMINA_DATA_DIR"),
            }
        }
    }

    #[test]
    fn script_escapes_credentials_and_respects_switch() {
        let _guard = EnvGuard::new("uis-autofill");
        let store = crate::store::Store::open().expect("临时库");
        store.set_setting(USER_KEY, "26303050173").unwrap();
        let protected = crate::secrets::protect("p\"a\\ss").expect("DPAPI 可用");
        store.set_setting(PASS_KEY, &protected).unwrap();

        let injected = script(&store);
        assert!(injected.contains("26303050173"), "学号应当写进脚本");
        // 密码里的引号 / 反斜杠必须被 JSON 转义，否则脚本会语法错误
        assert!(injected.contains(r#"p\"a\\ss"#), "密码应当 JSON 转义");
        assert!(!injected.contains("__USER__"), "占位符应当被替换");

        // 关掉开关就不注入
        store.set_setting(AUTOFILL_KEY, "0").unwrap();
        assert!(script(&store).is_empty(), "关掉自动填表应当返回空串");

        // 没有密码也不注入
        store.set_setting(AUTOFILL_KEY, "1").unwrap();
        store.set_setting(PASS_KEY, "").unwrap();
        assert!(script(&store).is_empty(), "没有密码时应当返回空串");
    }

    #[test]
    fn login_hosts_are_recognized() {
        assert!(is_login_host("id.fudan.edu.cn"));
        assert!(is_login_host("UIS.FUDAN.EDU.CN"));
        assert!(!is_login_host("fdjwgl.fudan.edu.cn"));
        assert!(!is_login_host("elearning.fudan.edu.cn"));
        assert!(!is_login_host(""));
    }
}
