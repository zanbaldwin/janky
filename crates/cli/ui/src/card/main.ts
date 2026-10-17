import "./card.css";
import { forwardFlips, show, type CardData } from "./reviewer";

const element = document.getElementById("janky-card");
if (!element) throw new Error("missing #janky-card");
forwardFlips();
void show(JSON.parse(element.textContent ?? "") as CardData);
