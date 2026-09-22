export function createUpdateFeed({ repository, version, assets, signatures }) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository) || !/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) throw new Error('Invalid update release identity');
  const select = (suffix, excluded) => {
    const matches = assets.filter(({ name }) => name.endsWith(suffix) && (!excluded || !name.endsWith(excluded)));
    if (matches.length !== 1) throw new Error(`Expected one update artifact: ${suffix}`);
    const { name } = matches[0];
    const signature = signatures.get(`${name}.sig`)?.trim();
    if (!signature || !/^[A-Za-z0-9+/]+={0,2}$/.test(signature)) throw new Error(`Missing or invalid update signature: ${name}`);
    return { url: `https://github.com/${repository}/releases/download/${version}/${encodeURIComponent(name)}`, signature };
  };
  const macos = select('.app.tar.gz');
  const linux = select('.AppImage');
  const msi = select('.msi', '-machine.msi');
  const machine = select('-machine.msi');
  const nsis = select('-nsis.exe');
  return { version: version.slice(1), notes: 'A signed Muniment desktop update.', platforms: {
    'darwin-aarch64': macos, 'darwin-x86_64': macos,
    'linux-x86_64': linux, 'linux-x86_64-appimage': linux,
    'windows-x86_64': nsis, 'windows-x86_64-nsis': nsis, 'windows-x86_64-msi': msi, 'windows-x86_64-msi-user': msi, 'windows-x86_64-msi-machine': machine,
  } };
}
