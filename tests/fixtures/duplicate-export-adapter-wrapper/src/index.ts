import { configureAuth as configureCiAuth } from "./ci/auth";
import { configureAuth } from "./gitlab/auth";
import { Package } from "./js/package";
import { format as formatA } from "./control/a";
import { format as formatB } from "./control/b";

configureCiAuth("https://ci.example");
configureAuth({ url: "https://gitlab.example" });
new Package().build();
console.log(formatA(1), formatB(2));
