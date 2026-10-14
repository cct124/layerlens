// Windows PowerShell 5.1 的模块路径不能继承中间进程携带的 PowerShell 7 路径。
// https://learn.microsoft.com/powershell/module/microsoft.powershell.core/about/about_psmodulepath

/** 仅为子进程恢复默认模块搜索路径，不修改父进程或用户／系统环境。 */
export function windowsPowerShellEnvironment(parent = process.env) {
  return Object.fromEntries(
    Object.entries(parent).filter(([name]) => name.toUpperCase() !== 'PSMODULEPATH'),
  );
}
