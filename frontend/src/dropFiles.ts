// Files dropped on the page, walking into dropped folders.

type Entry = FileSystemEntry

function readEntry(entry: Entry, out: File[]): Promise<void> {
  return new Promise((resolve) => {
    if (entry.isFile) {
      ;(entry as FileSystemFileEntry).file(
        (f) => {
          out.push(f)
          resolve()
        },
        () => resolve(),
      )
    } else if (entry.isDirectory) {
      const reader = (entry as FileSystemDirectoryEntry).createReader()
      const step = () =>
        reader.readEntries(
          (entries) => {
            if (!entries.length) return resolve()
            Promise.all(entries.map((e) => readEntry(e, out))).then(step)
          },
          () => resolve(),
        )
      step()
    } else {
      resolve()
    }
  })
}

/** Every file dropped, walking into dropped folders. */
export async function droppedFiles(data: DataTransfer): Promise<File[]> {
  const items = [...(data.items ?? [])]
  if (!items.length || !items[0].webkitGetAsEntry) return [...data.files]
  const out: File[] = []
  const entries = items.map((i) => i.webkitGetAsEntry()).filter((e): e is Entry => !!e)
  await Promise.all(entries.map((e) => readEntry(e, out)))
  return out
}
