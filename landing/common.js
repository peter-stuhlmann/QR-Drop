// Helpers shared by both phone pages. The server inlines this file at the
// top of each page's script, inside its function scope (no globals).

/** UI language: German on German phones, English everywhere else. */
var lang = (navigator.language || "de").toLowerCase().indexOf("de") === 0 ? "de" : "en";

/** Path of this share, e.g. "/s/<token>"; every API call starts here. */
var sharePath = location.pathname.replace(/\/+$/, "");

/** Texts both pages use; a page can override any of them. */
var SHARED_TEXTS = {
  de: {
    goneTitle: "Dieser Link ist nicht mehr gültig",
    offline: "Keine Verbindung zum Computer.",
    local: "Direkt über dein WLAN – nichts geht ins Internet.",
  },
  en: {
    goneTitle: "This link is no longer valid",
    offline: "No connection to the computer.",
    local: "Sent directly over your Wi-Fi – nothing goes to the internet.",
  },
};

function byId(id) {
  return document.getElementById(id);
}

/** Picks the page's texts in the UI language, adds the shared ones and fills
    [data-t] elements (text) and [data-ph] fields (placeholder). */
function setUpTexts(pageTexts) {
  var texts = pageTexts[lang];
  var shared = SHARED_TEXTS[lang];
  for (var key in shared) {
    if (!(key in texts)) texts[key] = shared[key];
  }
  document.documentElement.lang = lang;
  var labelled = document.querySelectorAll("[data-t]");
  for (var i = 0; i < labelled.length; i++) {
    labelled[i].textContent = texts[labelled[i].getAttribute("data-t")];
  }
  var fields = document.querySelectorAll("[data-ph]");
  for (var j = 0; j < fields.length; j++) {
    fields[j].placeholder = texts[fields[j].getAttribute("data-ph")];
  }
  return texts;
}

/** The page data the server put into #share-data; {} if missing or broken. */
function readShareData() {
  try {
    var data = JSON.parse(byId("share-data").textContent || "{}");
    return data && typeof data === "object" ? data : {};
  } catch (e) {
    return {};
  }
}

/* Intl.NumberFormat is costly to create and uploads format sizes several
   times a second: keep one per number of decimals. */
var numberFormats = {};

/** "1,4 MB" style sizes (decimal units, like the phone's own file apps). */
function formatSize(bytes) {
  var units = ["B", "KB", "MB", "GB", "TB"];
  var unit = 0;
  var value = bytes;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  var digits = unit === 0 || value >= 100 ? 0 : 1;
  if (!numberFormats[digits]) {
    numberFormats[digits] = new Intl.NumberFormat(lang, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    });
  }
  return numberFormats[digits].format(value) + " " + units[unit];
}

/** Replaces every card with the "link no longer valid" card. */
function showGoneCard(title, text) {
  var cards = document.querySelectorAll(".card");
  for (var i = 0; i < cards.length; i++) cards[i].hidden = cards[i].id !== "gone";
  byId("goneTitle").textContent = title;
  byId("goneText").textContent = text;
}
