// Reassembles the email links written by _includes/email_link.liquid. The page
// carries the address's two halves reversed and apart, so it never appears
// whole in the source; here they become a working mailto link.

function reverse(text) {
  return Array.from(text || "").reverse().join("");
}

export function initEmailLinks() {
  document.querySelectorAll("[data-email-user][data-email-domain]").forEach((link) => {
    const address = `${reverse(link.getAttribute("data-email-user"))}@${reverse(link.getAttribute("data-email-domain"))}`;
    link.href = `mailto:${address}`;
    link.textContent = address;
  });
}
