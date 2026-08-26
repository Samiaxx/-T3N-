import { deploy } from "./deploy";
import { grant } from "./grant";
import { render, send } from "./invoke";
import { connect } from "./session";
import { config } from "./env";

const commands: Record<string, () => Promise<void>> = {
  async whoami() {
    const { did, nodeUrl } = await connect();
    console.log(`env=${config.env}`);
    console.log(`node=${nodeUrl}`);
    console.log(`tenant DID=${did}`);
  },
  deploy,
  grant,
  render,
  send,
  async all() {
    await deploy();
    console.log("\n----------------------------------------\n");
    await grant();
    console.log("\n----------------------------------------\n");
    await render();
    console.log("\n----------------------------------------\n");
    await send();
  },
};

const cmd = process.argv[2];
const fn = cmd ? commands[cmd] : undefined;

if (!fn) {
  console.error(`Usage: tsx src/cli.ts <command>`);
  console.error(`Commands: ${Object.keys(commands).join(", ")}`);
  process.exit(1);
}

fn()
  .then(() => process.exit(0))
  .catch((e) => {
    console.error("\nERROR:", e?.message || e);
    process.exit(1);
  });
