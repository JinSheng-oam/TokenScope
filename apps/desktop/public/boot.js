(function () {
  try {
    var mode = window.localStorage.getItem('tokenscope.theme');
    var isDark =
      mode === 'light'
        ? false
        : mode === 'system'
          ? window.matchMedia('(prefers-color-scheme: dark)').matches
          : true;
    document.documentElement.setAttribute('data-theme', isDark ? 'dark' : 'light');
  } catch (_) {}
})();
