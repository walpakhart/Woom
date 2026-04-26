// Static metadata + shared types.

export type ConnectionCategory = 'sources' | 'agents';

export interface ConnectionMeta {
  id: string;
  name: string;
  desc: string;
  /** Inline SVG markup (paths, polygons, etc.) rendered inside a 24×24
      viewBox — single-color (uses `currentColor`) so the icon inherits the
      card tint. Falls back to `iconLetters` if omitted. */
  iconSvg?: string;
  /** Optional raster brand mark (PNG in /static). Used when the official
   *  logo can't be cleanly distilled into a single mono `<path>` —
   *  Cursor's gradient-shaded 3D hexagon, Claude's coral 12-spike
   *  asterisk, etc. Renderers prefer `iconImg` over `iconSvg` when set. */
  iconImg?: string;
  iconLetters: string;
  iconClass: string;
  category: ConnectionCategory;
  kind: string;
  implemented: boolean;
}

// Brand marks — sourced from simple-icons (CC0 / MIT), simplified to single
// `<path>` runs where possible. `currentColor` fill keeps them themeable.
const SVG_GITHUB = '<path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.4 3-.405 1.02.005 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12"/>';
const SVG_JIRA = '<path d="M11.571 11.513H0a5.215 5.215 0 0 0 5.215 5.215h2.132v2.066A5.215 5.215 0 0 0 12.562 24V12.504a.99.99 0 0 0-.991-.991Zm5.723-5.756H5.736a5.215 5.215 0 0 0 5.215 5.214h2.132v2.067a5.215 5.215 0 0 0 5.214 5.214V6.748a.99.99 0 0 0-.991-.991Zm5.715-5.757H11.45a5.215 5.215 0 0 0 5.215 5.215h2.132v2.066A5.215 5.215 0 0 0 24 12.495V.991A.99.99 0 0 0 23.01 0Z"/>';
const SVG_SLACK = '<path d="M5.042 15.165a2.528 2.528 0 0 1-2.52 2.523A2.528 2.528 0 0 1 0 15.165a2.527 2.527 0 0 1 2.522-2.52h2.52v2.52zm1.271 0a2.527 2.527 0 0 1 2.521-2.52 2.527 2.527 0 0 1 2.521 2.52v6.313A2.528 2.528 0 0 1 8.834 24a2.528 2.528 0 0 1-2.521-2.522v-6.313zM8.834 5.042a2.528 2.528 0 0 1-2.521-2.52A2.528 2.528 0 0 1 8.834 0a2.528 2.528 0 0 1 2.521 2.522v2.52H8.834zm0 1.271a2.527 2.527 0 0 1 2.521 2.521 2.527 2.527 0 0 1-2.521 2.521H2.522A2.527 2.527 0 0 1 0 8.834a2.527 2.527 0 0 1 2.522-2.521h6.312zm10.122 2.521a2.528 2.528 0 0 1 2.522-2.521A2.528 2.528 0 0 1 24 8.834a2.528 2.528 0 0 1-2.522 2.521h-2.522V8.834zm-1.268 0a2.527 2.527 0 0 1-2.523 2.521 2.527 2.527 0 0 1-2.52-2.521V2.522A2.527 2.527 0 0 1 15.165 0a2.528 2.528 0 0 1 2.523 2.522v6.312zm-2.523 10.122a2.528 2.528 0 0 1 2.523 2.522A2.528 2.528 0 0 1 15.165 24a2.527 2.527 0 0 1-2.52-2.522v-2.522h2.52zm0-1.268a2.527 2.527 0 0 1-2.52-2.523 2.526 2.526 0 0 1 2.52-2.52h6.313A2.527 2.527 0 0 1 24 15.165a2.528 2.528 0 0 1-2.522 2.523h-6.313z"/>';
const SVG_LINEAR = '<path d="M3.007 11.98c0-.12.144-.181.23-.097l9.88 9.88a.137.137 0 0 1-.098.23 10.04 10.04 0 0 1-4.856-1.25 10.05 10.05 0 0 1-5.044-8.763ZM3.236 8.132a.26.26 0 0 1 .046-.3l4.55-4.549a.26.26 0 0 1 .3-.047 10.05 10.05 0 0 1 12.631 12.632.26.26 0 0 1-.047.3l-4.55 4.549a.26.26 0 0 1-.3.046A10.06 10.06 0 0 1 3.236 8.132Zm.69-2.58a.13.13 0 0 1 .005-.184 10.04 10.04 0 0 1 2.63-1.872.13.13 0 0 1 .156.02l11.768 11.767a.13.13 0 0 1 .02.156 10.05 10.05 0 0 1-1.872 2.63.13.13 0 0 1-.185.005L3.926 5.552ZM8.132 3.236a10.05 10.05 0 0 1 2.38-.236A10.05 10.05 0 0 1 21 13.488a10.05 10.05 0 0 1-.236 2.38.13.13 0 0 1-.219.066L8.066 3.455a.13.13 0 0 1 .066-.22Z"/>';
const SVG_NOTION = '<path d="M4.459 4.208c.746.606 1.026.56 2.428.466l13.215-.793c.28 0 .047-.28-.046-.326L17.86 1.968c-.42-.326-.981-.7-2.055-.607L3.01 2.295c-.466.046-.56.28-.374.466zm.793 3.08v13.904c0 .747.373 1.027 1.214.98l14.523-.84c.841-.046.935-.56.935-1.167V6.354c0-.606-.233-.933-.748-.887l-15.177.887c-.56.047-.747.327-.747.933zm14.337.745c.093.42 0 .84-.42.888l-.7.14v10.264c-.608.327-1.168.514-1.635.514-.748 0-.935-.234-1.495-.933l-4.577-7.186v6.952L12.21 19s0 .84-1.168.84l-3.222.186c-.093-.186 0-.653.327-.746l.84-.233V9.854L7.822 9.76c-.094-.42.14-1.026.793-1.073l3.456-.233 4.764 7.279v-6.44l-1.215-.139c-.093-.514.28-.887.747-.933zM1.936 1.035l13.31-.98c1.634-.14 2.055-.047 3.082.7l4.249 2.986c.7.513.934.653.934 1.213v16.378c0 1.026-.373 1.634-1.68 1.726l-15.458.934c-.98.047-1.448-.093-1.962-.747l-3.129-4.06C.699 18.42.5 17.86.5 17.207V2.667c0-.839.374-1.54 1.436-1.632z"/>';
const SVG_GITLAB = '<path d="m23.6 9.593-.033-.087L20.3.98a.848.848 0 0 0-.332-.394.87.87 0 0 0-.993.052.867.867 0 0 0-.289.45l-2.204 6.773H7.518L5.314 1.088a.858.858 0 0 0-.29-.452.87.87 0 0 0-.992-.052.87.87 0 0 0-.332.395L.42 9.503l-.033.087a6.05 6.05 0 0 0 2.003 6.977l.013.01.03.022 4.95 3.704 2.451 1.852 1.49 1.127a1.028 1.028 0 0 0 1.241 0l1.492-1.127 2.45-1.852 4.98-3.726.014-.01A6.05 6.05 0 0 0 23.6 9.593Z"/>';
const SVG_TEAMS = '<path d="M20.625 8.127q.45 0 .85.174t.706.475q.306.3.475.699.174.399.175.85v5.75q0 .45-.175.85-.17.399-.476.698-.3.301-.7.476-.4.174-.85.175h-4.25q-.45 0-.85-.175-.399-.175-.699-.476-.301-.3-.476-.699-.174-.399-.175-.85V9.127h-5v8.5h-4q-.599 0-1.125-.225t-.925-.625q-.4-.4-.625-.926T3.375 14.751q0-.425.108-.823t.308-.76q.2-.362.476-.674.276-.313.6-.552V7.373q0-.6.225-1.125t.624-.924q.399-.401.926-.625.526-.226 1.125-.225h8q.6 0 1.125.225.525.224.924.624t.625.925q.226.526.225 1.125v.75h4.083zM13.12 6.751H6.62v3.75h1.375v-1.75h1.75v1.75h1.375v-1.75h1.75v-1.75H13.12zM9.12 17.627a2 2 0 0 1 .2-.875 2.3 2.3 0 0 1 .55-.726 2.7 2.7 0 0 1 .8-.5q.45-.175.95-.175h1v-3h-5v5h1.5zm9.505-8.5q-.88 0-1.5.617t-.62 1.508q0 .87.62 1.497.62.628 1.51.628.898 0 1.515-.626.617-.625.617-1.499t-.617-1.5a2.05 2.05 0 0 0-1.525-.625zm0 12q.88 0 1.49-.615.609-.615.61-1.51 0-.87-.61-1.497a2.06 2.06 0 0 0-1.5-.628q-.896 0-1.506.625a2.05 2.05 0 0 0-.609 1.5q0 .87.614 1.497t1.521.628z"/>';
const SVG_ASANA = '<path d="M18.7809 11.5528c-2.88 0-5.2192 2.3392-5.2192 5.2237 0 2.8799 2.3392 5.2192 5.2192 5.2192s5.2191-2.3393 5.2191-5.2192c0-2.8845-2.3392-5.2237-5.2191-5.2237ZM5.2192 11.5528C2.3393 11.5528 0 13.892 0 16.7765 0 19.6564 2.3392 22 5.2192 22s5.2192-2.3393 5.2192-5.2192c0-2.8845-2.3392-5.2237-5.2192-5.2237ZM17.2192 5.225c0 2.8799-2.3392 5.2191-5.2192 5.2191-2.8845 0-5.2192-2.3392-5.2192-5.2191C6.7808 2.3405 9.1156 0 12 0c2.88 0 5.2192 2.345 5.2192 5.225Z"/>';
/* Anthropic "A" silhouette — clean A with the inner triangle knocked
   out via fill-rule="evenodd". Replaces the old path that drew an
   "A I" pair (looked like a stylised "AI"). */
const SVG_CLAUDE = '<path fill-rule="evenodd" d="M3 21L11 3h2l8 18h-3l-1.5-3.5h-9L6 21H3zm5.5-6h7L12 6.7 8.5 15z"/>';
const SVG_CURSOR = '<path d="M11.925 24l10.425-6-10.425-6L1.5 18zm0-24L1.5 6 11.925 12 22.35 6Zm0 0l10.425 6V6.001L11.925 0Zm-10.425 6v12L11.925 12Zm20.85 0v12l-10.425-6Z"/>';
const SVG_OPENAI = '<path d="M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.4069-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z"/>';
const SVG_AIDER = '<path d="M12 2 3 7v10l9 5 9-5V7l-9-5zm0 2.311 6.5 3.611v3.189l-3.75-2.083L12 10.578 9.25 9.028 5.5 11.111V7.922L12 4.311zm0 4.889 3 1.667-3 1.667-3-1.667 3-1.667zM5.5 13.355l3.25 1.805v3.723L5.5 17.078v-3.723zm13 0v3.723l-3.25 1.805V15.16L18.5 13.355zm-7.75 1.805v3.723L12 19.689l1.25-.806V15.16L12 14.456l-1.25.704z"/>';
const SVG_SENTRY = '<path d="M12.74.48a1.74 1.74 0 0 0-2.43.62L7.83 5.36a13.83 13.83 0 0 1 8.38 11.59h-2.36a11.5 11.5 0 0 0-7.21-9.59L4 12.09a6.07 6.07 0 0 1 4.27 4.86H1.78a.43.43 0 0 1-.37-.65l1.55-2.66a5.4 5.4 0 0 0-1.7-1L0 15.31a2.27 2.27 0 0 0 1.95 3.41h8.56a8.34 8.34 0 0 0-3.32-7.71l1.18-2a10.51 10.51 0 0 1 4.31 9.74h5.82a12.62 12.62 0 0 0-6.05-11.07l2.43-4.16a.42.42 0 0 1 .58-.14.43.43 0 0 1 .15.16l9.56 16.45a.42.42 0 0 1-.37.65h-2.32q.06 1.13 0 2.26h2.36a2.78 2.78 0 0 0 2.41-4.17z"/>';
const SVG_COPILOT = '<path d="M23.22 11.55c-.017-.109-.036-.217-.058-.325a7.17 7.17 0 0 0-.276-.992 5.66 5.66 0 0 0-.47-.992 4.16 4.16 0 0 0-.704-.854 3.48 3.48 0 0 0-.993-.634 3.87 3.87 0 0 0-1.322-.253c-.46-.01-.91.033-1.339.13-.422.096-.83.234-1.218.416.011-.19.011-.379 0-.567a6.08 6.08 0 0 0-.18-1.04 5.25 5.25 0 0 0-.402-1.04 4.01 4.01 0 0 0-.693-.93 3.29 3.29 0 0 0-1.02-.676 3.51 3.51 0 0 0-1.386-.245c-.44 0-.865.06-1.273.18a6.3 6.3 0 0 0-1.131.47c-.36.195-.702.42-1.025.672-.323.252-.623.53-.897.833a5.05 5.05 0 0 0-.676.938 3.35 3.35 0 0 0-.393.91l-.094-.038a4.42 4.42 0 0 0-1.386-.324 3.47 3.47 0 0 0-1.446.21 3.29 3.29 0 0 0-1.16.768 3.45 3.45 0 0 0-.735 1.17c-.168.438-.257.9-.264 1.37-.007.47.059.937.195 1.381.13.437.329.85.588 1.226a4.76 4.76 0 0 0 .925.98c-.176.336-.314.689-.411 1.055a3.94 3.94 0 0 0-.141 1.142c.018.388.088.77.209 1.14.124.365.301.712.527 1.028.225.317.502.593.819.81.32.218.674.375 1.05.466.38.09.772.11 1.16.059.391-.05.775-.168 1.137-.35a4.98 4.98 0 0 0 1.022-.697c.313-.279.589-.596.82-.945.38.187.783.322 1.2.402.418.078.845.099 1.267.062.42-.036.836-.13 1.237-.278.401-.152.78-.362 1.126-.624.345-.263.645-.58.891-.942.249-.366.438-.77.56-1.195a4.99 4.99 0 0 0 .192-1.314c-.002-.451-.082-.897-.236-1.319a4.26 4.26 0 0 0-.684-1.196c.377-.138.74-.316 1.082-.532.35-.223.67-.49.953-.797.283-.304.529-.646.73-1.014.206-.38.352-.795.435-1.225.086-.45.1-.914.041-1.369zm-14.373 8.196a2.07 2.07 0 0 1-.852-.178 2.1 2.1 0 0 1-.67-.487 2.23 2.23 0 0 1-.432-.729 2.39 2.39 0 0 1-.134-.88c.014-.307.083-.607.204-.892.127-.287.304-.548.52-.77a2.75 2.75 0 0 1 1.75-.79c.328-.018.655.02.968.112.314.094.613.245.88.446.265.198.495.445.68.728.188.284.328.6.415.934.085.33.119.672.1 1.01a2.83 2.83 0 0 1-.248.961 2.84 2.84 0 0 1-.56.824 2.51 2.51 0 0 1-1.72.711zm7.24-.35a2.5 2.5 0 0 1-1.135.262 2.48 2.48 0 0 1-1.133-.272 2.43 2.43 0 0 1-.892-.731 2.45 2.45 0 0 1-.47-1.104 2.47 2.47 0 0 1 .123-1.204 2.51 2.51 0 0 1 .646-1.02 2.56 2.56 0 0 1 1.064-.619c.398-.118.82-.132 1.224-.041.405.092.785.279 1.1.541.316.262.559.605.704.99.145.387.19.806.13 1.218a2.48 2.48 0 0 1-.406 1.073 2.49 2.49 0 0 1-.955.907zm4.148-3.794c-.204.198-.43.37-.677.512a3.19 3.19 0 0 1-.77.316 3.07 3.07 0 0 1-.817.098 3.17 3.17 0 0 1-.824-.118 3.86 3.86 0 0 1-.812-.335 5.09 5.09 0 0 1-.75-.529 6.96 6.96 0 0 1-.71-.7 4.08 4.08 0 0 1-.549-.862 4.13 4.13 0 0 1-.329-.995 3.75 3.75 0 0 1-.075-1.036 3.5 3.5 0 0 1 .207-1.016c.134-.32.314-.617.535-.883a3.27 3.27 0 0 1 .815-.693 3.1 3.1 0 0 1 1.03-.401 3.61 3.61 0 0 1 1.12-.042 3.33 3.33 0 0 1 1.072.326c.325.16.619.372.867.626.252.254.454.547.601.864.15.32.25.659.298.999.046.34.038.683-.027 1.02-.063.339-.18.664-.35.96-.167.297-.376.566-.624.799z"/>';

export const connectionsMeta: ConnectionMeta[] = [
  { id: 'github', name: 'GitHub', desc: 'Pull requests, issues, review comments.', iconSvg: SVG_GITHUB, iconLetters: 'GH', iconClass: 'conn-icon--github', category: 'sources', kind: 'personal access token', implemented: true },
  { id: 'jira', name: 'Jira', desc: 'Tickets, comments, workflow transitions.', iconSvg: SVG_JIRA, iconLetters: 'J', iconClass: 'conn-icon--jira', category: 'sources', kind: 'atlassian api token', implemented: true },
  { id: 'sentry', name: 'Sentry', desc: 'Errors, issues, events — drop onto Claude to debug.', iconSvg: SVG_SENTRY, iconLetters: 'St', iconClass: 'conn-icon--sentry', category: 'sources', kind: 'auth token', implemented: true },
  { id: 'slack', name: 'Slack', desc: 'Channels, threads, reminders.', iconSvg: SVG_SLACK, iconLetters: 'S', iconClass: 'conn-icon--slack', category: 'sources', kind: 'oauth', implemented: false },
  { id: 'linear', name: 'Linear', desc: 'Issues, projects, cycles.', iconSvg: SVG_LINEAR, iconLetters: 'L', iconClass: 'conn-icon--linear', category: 'sources', kind: 'oauth', implemented: false },
  { id: 'notion', name: 'Notion', desc: 'Pages and databases as work objects.', iconSvg: SVG_NOTION, iconLetters: 'N', iconClass: 'conn-icon--notion', category: 'sources', kind: 'oauth', implemented: false },
  { id: 'gitlab', name: 'GitLab', desc: 'Merge requests and issues.', iconSvg: SVG_GITLAB, iconLetters: 'Gl', iconClass: 'conn-icon--gitlab', category: 'sources', kind: 'oauth', implemented: false },
  { id: 'teams', name: 'MS Teams', desc: 'Channels, chats, mentions.', iconSvg: SVG_TEAMS, iconLetters: 'T', iconClass: 'conn-icon--teams', category: 'sources', kind: 'graph api', implemented: false },
  { id: 'asana', name: 'Asana', desc: 'Tasks and assignments.', iconSvg: SVG_ASANA, iconLetters: 'As', iconClass: 'conn-icon--asana', category: 'sources', kind: 'oauth', implemented: false },
  { id: 'claude', name: 'Claude Code', desc: 'Headless coding agent in a worktree.', iconSvg: SVG_CLAUDE, iconImg: '/brand-claude.png', iconLetters: 'C', iconClass: 'conn-icon--claude', category: 'agents', kind: 'claude CLI', implemented: true },
  { id: 'cursor', name: 'Cursor', desc: 'cursor-agent CLI as an alternate agent.', iconSvg: SVG_CURSOR, iconImg: '/brand-cursor.png', iconLetters: 'Cr', iconClass: 'conn-icon--cursor', category: 'agents', kind: 'cursor-agent', implemented: true },
  { id: 'codex', name: 'Codex CLI', desc: 'OpenAI Codex CLI for coding runs.', iconSvg: SVG_OPENAI, iconLetters: 'Cd', iconClass: 'conn-icon--codex', category: 'agents', kind: 'codex', implemented: false },
  { id: 'aider', name: 'Aider', desc: 'Open-source AI pair programmer.', iconSvg: SVG_AIDER, iconLetters: 'A', iconClass: 'conn-icon--aider', category: 'agents', kind: 'aider', implemented: false },
  { id: 'copilot', name: 'GitHub Copilot', desc: 'Copilot CLI for shell-mode suggestions.', iconSvg: SVG_COPILOT, iconLetters: 'Co', iconClass: 'conn-icon--copilot', category: 'agents', kind: 'gh copilot', implemented: false }
];

export interface GithubUser {
  login: string;
  id: number;
  name: string | null;
  avatar_url: string;
}

export type ConnectionStatus =
  | { kind: 'disconnected' }
  | { kind: 'connected'; user: GithubUser };

export interface JiraUser {
  account_id: string;
  email_address: string | null;
  display_name: string;
  avatar_url: string;
  workspace: string;
}

export type JiraStatus =
  | { kind: 'disconnected' }
  | { kind: 'connected'; user: JiraUser };

export interface ClaudeStatus {
  detected: boolean;
  path: string | null;
  version: string | null;
  has_config_dir: boolean;
  has_api_key_env: boolean;
  ready: boolean;
}

export interface CursorStatus {
  detected: boolean;
  path: string | null;
  version: string | null;
  has_config_dir: boolean;
  has_api_key_env: boolean;
  ready: boolean;
}

export interface AgentStatus {
  claude: ClaudeStatus;
  cursor: CursorStatus;
}

export interface JiraActor {
  display_name: string;
  avatar_url: string | null;
  account_id: string | null;
}

export interface JiraUserSummary {
  account_id: string;
  display_name: string;
  email_address: string | null;
  avatar_url: string;
  active: boolean;
}

export interface JiraItem {
  id: string;
  key: string;
  summary: string;
  description: string | null;
  status: string;
  status_category: string; // "new" | "indeterminate" | "done"
  priority: string | null;
  issue_type: string;
  assignee: JiraActor | null;
  reporter: JiraActor | null;
  labels: string[];
  updated: string;
  created: string;
  url: string;
}

// ---------- Sentry ----------

export interface SentryUser {
  id: string;
  email: string | null;
  username: string | null;
  name: string | null;
  organization_slug: string;
  organization_name: string;
  /** Base URL of the Sentry instance — `https://sentry.io` for cloud,
   *  custom URL for self-hosted. Stored alongside the token so we can
   *  reach the same install on every API call. */
  host: string;
}

export type SentryStatus =
  | { kind: 'disconnected' }
  | { kind: 'connected'; user: SentryUser };

/** A single Sentry issue (a group of similar events). The shape mirrors
 *  what `/api/0/organizations/{org}/issues/` returns, simplified. */
export interface SentryIssue {
  id: string;
  /** "PROJ-123" — short stable ref the user copies into commit messages. */
  short_id: string;
  title: string;
  culprit: string | null;
  /** "error" | "warning" | "info" | "debug" | "fatal" */
  level: string;
  /** "unresolved" | "resolved" | "ignored" */
  status: string;
  platform: string | null;
  project_slug: string;
  project_name: string;
  /** Total occurrence count as a stringified number from the API. */
  count: string;
  user_count: number;
  first_seen: string;
  last_seen: string;
  permalink: string;
  /** Type + message — mirrors the bold/secondary line in Sentry's UI. */
  metadata_type: string | null;
  metadata_value: string | null;
}

export interface SentryProject {
  id: string;
  slug: string;
  name: string;
  platform: string | null;
  is_member: boolean;
}

export interface SentryEnvironment {
  id: string;
  name: string;
}

export interface SentryStackFrame {
  function: string | null;
  filename: string | null;
  abs_path: string | null;
  lineno: number | null;
  colno: number | null;
  in_app: boolean;
  context: { line: number; source: string }[];
}

export interface SentryException {
  type: string | null;
  value: string | null;
  module: string | null;
  frames: SentryStackFrame[];
}

/** One row in the per-issue event list — what `sentry_list_events`
 *  returns. Lighter than `SentryEventDetail` (no stack frames or
 *  breadcrumbs); used by the sidecar pane's "Other events" picker so
 *  the user can jump to a specific occurrence without leaving the app. */
export interface SentryEvent {
  event_id: string;
  date_created: string;
  message: string | null;
  platform: string | null;
  /** First-line summary of the exception type + value, when present. */
  exception_summary: string | null;
  permalink: string | null;
}

export interface SentryEventDetail {
  event_id: string;
  date_created: string;
  platform: string | null;
  message: string | null;
  culprit: string | null;
  user_email: string | null;
  user_id: string | null;
  user_ip: string | null;
  release: string | null;
  environment: string | null;
  tags: [string, string][];
  exceptions: SentryException[];
  breadcrumbs_summary: string | null;
  permalink: string | null;
}

/** Sentry level → which mini-tag style to use. */
export function sentryLevelClass(level: string): string {
  switch (level) {
    case 'fatal':
    case 'error':
      return 'tag--closed';
    case 'warning':
      return 'tag--draft';
    case 'info':
    case 'debug':
    default:
      return 'tag--open';
  }
}

export function jiraStatusClass(cat: string): string {
  switch (cat) {
    case 'done':
      return 'tag--closed';
    case 'indeterminate':
      return 'tag--draft';
    case 'new':
    default:
      return 'tag--open';
  }
}

export interface Actor {
  login: string;
  avatar_url: string;
}

export interface Label {
  name: string;
  color: string;
}

export interface RepoRef {
  owner: string;
  name: string;
}

export interface InboxItem {
  id: number;
  number: number;
  title: string;
  body: string | null;
  state: string;
  is_pull_request: boolean;
  draft: boolean;
  merged: boolean;
  url: string;
  author: Actor | null;
  labels: Label[];
  assignees: Actor[];
  repo: RepoRef | null;
  comments: number;
  created_at: string;
  updated_at: string;
}

export interface PrDetail {
  number: number;
  state: string;
  draft: boolean;
  merged: boolean;
  mergeable: boolean | null;
  mergeable_state: string | null;
  head_ref: string;
  base_ref: string;
  additions: number;
  deletions: number;
  changed_files: number;
  commits: number;
}

export interface ChangedFile {
  filename: string;
  status: string;
  additions: number;
  deletions: number;
  changes: number;
  patch: string | null;
}

export interface CommitEntry {
  sha: string;
  short_sha: string;
  message: string;
  author_name: string;
  author_login: string | null;
  author_avatar: string | null;
  author_date: string;
  url: string;
}

export interface CommitDetail {
  sha: string;
  short_sha: string;
  message: string;
  author_name: string;
  author_login: string | null;
  author_avatar: string | null;
  author_date: string;
  url: string;
  additions: number;
  deletions: number;
  total_changes: number;
  files: ChangedFile[];
}

export interface Review {
  id: number;
  user: Actor | null;
  state: string;
  body: string;
  submitted_at: string | null;
}

export interface Comment {
  id: number;
  user: Actor | null;
  body: string;
  created_at: string;
  updated_at: string;
}

export interface ReviewComment {
  id: number;
  user: Actor | null;
  body: string;
  path: string;
  line: number | null;
  original_line: number | null;
  side: string | null;
  commit_id: string;
  in_reply_to_id: number | null;
  pull_request_review_id: number | null;
  created_at: string;
  updated_at: string;
}

export interface Repository {
  id: number;
  name: string;
  full_name: string;
  owner: string;
  description: string | null;
  private: boolean;
  fork: boolean;
  archived: boolean;
  default_branch: string;
  stargazers_count: number;
  open_issues_count: number;
  language: string | null;
  updated_at: string;
  html_url: string;
}

export interface CheckRun {
  id: number;
  name: string;
  status: string;              // "queued" | "in_progress" | "completed"
  conclusion: string | null;   // "success" | "failure" | "neutral" | "cancelled" | "skipped" | "timed_out" | "action_required"
  details_url: string | null;
  app_name: string | null;
  started_at: string | null;
  completed_at: string | null;
}

export interface WorkflowRun {
  id: number;
  name: string;
  display_title: string;
  head_branch: string;
  head_sha: string;
  event: string;
  status: string;          // queued | in_progress | completed | …
  conclusion: string | null; // success | failure | cancelled | skipped | neutral | timed_out
  created_at: string;
  updated_at: string;
  html_url: string;
  actor_login: string | null;
  actor_avatar: string | null;
  run_number: number;
  workflow_id: number;
  run_attempt: number;
}

export interface RepoReadme {
  name: string;
  content: string;  // markdown source
  html_url: string;
}

export interface TreeEntry {
  path: string;
  sha: string;
  kind: string;       // 'blob' | 'tree' | 'commit' | 'notice'
  size: number | null;
}

export interface FileBlob {
  path: string;
  content: string;
  size: number;
  sha: string;
  encoding: string;
  is_text: boolean;
}

export interface Release {
  id: number;
  tag_name: string;
  name: string | null;
  body: string | null;
  draft: boolean;
  prerelease: boolean;
  created_at: string;
  published_at: string | null;
  html_url: string;
  author_login: string | null;
  author_avatar: string | null;
}

export interface RepoCommit {
  sha: string;
  short_sha: string;
  message: string;
  author_name: string;
  author_login: string | null;
  author_avatar: string | null;
  date: string;
  html_url: string;
}

export interface RepoBranch {
  name: string;
  sha: string;
  protected: boolean;
}

export interface CompareResult {
  total_commits: number;
  ahead_by: number;
  behind_by: number;
  additions: number;
  deletions: number;
  commits: CommitEntry[];
  files: ChangedFile[];
}

export interface JiraProject {
  id: string;
  key: string;
  name: string;
  avatar_url: string | null;
}

export interface JiraBoard {
  id: number;
  name: string;
  type_: string; // "scrum" | "kanban" | "simple"
  project_key: string | null;
}

export interface JiraSprint {
  id: number;
  name: string;
  state: string; // "active" | "closed" | "future"
  board_id: number;
}

export interface JiraIssueType {
  id: string;
  name: string;
  subtask: boolean;
  icon_url: string | null;
}

export interface JiraComment {
  id: string;
  author: JiraActor | null;
  body: string;
  created: string;
  updated: string;
}

/** One native Jira worklog entry. Mirrors `JiraWorklog` in jira.rs.
 *  `time_spent` is Jira's own `"1h 30m"` label — kept verbatim so our UI
 *  matches what users see inside Jira/Tempo. */
export interface JiraWorklog {
  id: string;
  author: JiraActor | null;
  comment: string;
  created: string;
  updated: string;
  started: string;
  time_spent_seconds: number;
  time_spent: string;
}

export interface JiraTransition {
  id: string;
  name: string;
  to_status: string;
  to_status_category: string;
}

/** Workflow status — one row in the inbox status-filter dropdown. Mirrors
 *  the `JiraStatus` struct in `src-tauri/src/jira.rs`. (The connection
 *  status union up top owns the `JiraStatus` type name, so this richer
 *  workflow shape gets a distinct one to avoid the clash.) */
export interface JiraWorkflowStatus {
  id: string;
  name: string;
  /** One of `new`, `indeterminate`, `done`, `undefined`. */
  category_key: string;
  /** Jira's reported palette name (e.g. `blue-gray`, `yellow`, `green`,
   *  `medium-gray`). Mapped to hex by `jiraStatusColor`. */
  color: string;
}

/** Map Jira's `statusCategory.colorName` values onto accent hexes tuned for
 *  the dark theme. Falls back to the `category_key` palette if the color is
 *  unexpected. */
export function jiraStatusColor(s: { color?: string; category_key?: string }): string {
  const byName: Record<string, string> = {
    'blue-gray': '#60a5fa',
    blue: '#60a5fa',
    'medium-gray': '#8b96ab',
    gray: '#8b96ab',
    yellow: '#fcd34d',
    green: '#34d399',
    warm_red: '#fca5a5',
    'warm-red': '#fca5a5',
    red: '#fca5a5',
    purple: '#b199f6'
  };
  const fromName = s.color ? byName[s.color.toLowerCase()] : undefined;
  if (fromName) return fromName;
  switch (s.category_key) {
    case 'done':
      return '#34d399';
    case 'indeterminate':
      return '#fcd34d';
    case 'new':
      return '#60a5fa';
    default:
      return '#8b96ab';
  }
}

export interface JiraDetail {
  id: string;
  key: string;
  summary: string;
  description: string;
  status: string;
  status_category: string;
  priority: string | null;
  issue_type: string;
  assignee: JiraActor | null;
  reporter: JiraActor | null;
  labels: string[];
  updated: string;
  created: string;
  url: string;
  comments: JiraComment[];
  transitions: JiraTransition[];
}

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const WEEK = 7 * DAY;

export function relativeTime(iso: string, now: number = Date.now()): string {
  const t = new Date(iso).getTime();
  const diff = Math.max(0, now - t);
  if (diff < MINUTE) return 'now';
  if (diff < HOUR) return `${Math.round(diff / MINUTE)}m`;
  if (diff < DAY) return `${Math.round(diff / HOUR)}h`;
  if (diff < WEEK) return `${Math.round(diff / DAY)}d`;
  return `${Math.round(diff / WEEK)}w`;
}

export type TimeGroup = 'today' | 'yesterday' | 'earlier';

export interface GroupedInbox {
  today: InboxItem[];
  yesterday: InboxItem[];
  earlier: InboxItem[];
}

export function groupByTime(items: InboxItem[], now: number = Date.now()): GroupedInbox {
  const result: GroupedInbox = { today: [], yesterday: [], earlier: [] };
  for (const item of items) {
    const diff = now - new Date(item.updated_at).getTime();
    if (diff < DAY) result.today.push(item);
    else if (diff < 2 * DAY) result.yesterday.push(item);
    else result.earlier.push(item);
  }
  return result;
}

export function repoLabel(item: InboxItem): string {
  if (item.repo) return `${item.repo.owner}/${item.repo.name}`;
  return '';
}

export function externalId(item: InboxItem): string {
  return `#${item.number}`;
}

export function stateTag(item: InboxItem): { text: string; className: string } {
  if (item.is_pull_request) {
    if (item.merged) return { text: 'merged', className: 'tag--merged' };
    if (item.draft) return { text: 'draft', className: 'tag--draft' };
    if (item.state === 'closed') return { text: 'closed', className: 'tag--closed' };
    return { text: 'open', className: 'tag--open' };
  }
  if (item.state === 'closed') return { text: 'closed', className: 'tag--closed' };
  return { text: 'open', className: 'tag--open' };
}

export function kindLabel(item: InboxItem): string {
  return item.is_pull_request ? 'PR' : 'issue';
}

// ---------- Diff parser ----------

export type DiffLineKind = 'context' | 'add' | 'del' | 'header';
export interface DiffLine {
  kind: DiffLineKind;
  text: string;
  oldLine?: number;
  newLine?: number;
}

export function parsePatch(patch: string): DiffLine[] {
  const lines = patch.split('\n');
  const result: DiffLine[] = [];
  let oldLine = 0;
  let newLine = 0;

  for (const raw of lines) {
    if (raw.startsWith('@@')) {
      const m = raw.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (m) {
        oldLine = Number(m[1]);
        newLine = Number(m[2]);
      }
      result.push({ kind: 'header', text: raw });
      continue;
    }
    if (raw.startsWith('+') && !raw.startsWith('+++')) {
      result.push({ kind: 'add', text: raw.slice(1), newLine });
      newLine += 1;
      continue;
    }
    if (raw.startsWith('-') && !raw.startsWith('---')) {
      result.push({ kind: 'del', text: raw.slice(1), oldLine });
      oldLine += 1;
      continue;
    }
    if (raw.startsWith('\\')) continue;
    result.push({ kind: 'context', text: raw.replace(/^ /, ''), oldLine, newLine });
    oldLine += 1;
    newLine += 1;
  }
  return result;
}

export function reviewStateLabel(s: string): { text: string; className: string } {
  switch (s) {
    case 'APPROVED':
      return { text: 'approved', className: 'rev--approved' };
    case 'CHANGES_REQUESTED':
      return { text: 'changes requested', className: 'rev--changes' };
    case 'COMMENTED':
      return { text: 'commented', className: 'rev--commented' };
    case 'DISMISSED':
      return { text: 'dismissed', className: 'rev--dismissed' };
    case 'PENDING':
      return { text: 'pending', className: 'rev--pending' };
    default:
      return { text: s.toLowerCase(), className: 'rev--commented' };
  }
}

// ---------- Repo helpers ----------

export function repoKey(r: RepoRef): string {
  return `${r.owner}/${r.name}`;
}
