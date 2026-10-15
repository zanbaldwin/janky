import "./card.css";
import { show, type CardData } from "./reviewer";

const element = document.getElementById("janky-card");
if (!element) throw new Error("missing #janky-card");
void show(JSON.parse(element.textContent ?? "") as CardData);
