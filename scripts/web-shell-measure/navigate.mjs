import { connectPage } from "./cdp.mjs";

const url = await new Promise((resolve, reject) => {
  let input = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk) => { input += chunk; });
  process.stdin.on("end", () => resolve(input));
  process.stdin.on("error", reject);
});
const page = await connectPage(process.argv[2], false);
try {
  await page.send("Page.navigate", { url });
} finally {
  page.close();
}
