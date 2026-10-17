/**
 * select / multi_select の札の色の数。Notion の選択肢の色 (default を除く gray・brown・orange・yellow・green・blue・
 * purple・pink・red の 9 色) に合わせた。色の値は styles.css の `.tag-color-<番号>`。
 */
const TAG_COLOR_COUNT = 9;

/**
 * 選択肢の名前 `optionName` の札の色の番号 (0 から `TAG_COLOR_COUNT` - 1)。選択肢は色を DB に持たないため名前から決め、
 * 表・詳細ページ・別のタブのどこでも同じ選択肢を同じ色にする。
 */
export function tagColor(optionName: string): number {
  let hash = 0;
  for (const character of optionName) {
    hash = (hash * 31 + (character.codePointAt(0) ?? 0)) % 1_000_003;
  }
  return hash % TAG_COLOR_COUNT;
}
