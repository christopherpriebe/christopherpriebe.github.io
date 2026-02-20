const webpack = require("webpack");
const path = require("path");
const MiniCssExtractPlugin = require("mini-css-extract-plugin");

module.exports = (env, argv) => {
  const isProduction = argv.mode === "production";

  return {
    entry: "./_js/index.js",
    output: {
      path: path.resolve(__dirname, "assets/js"),
      filename: "bundle.js",
    },
    plugins: [
      new MiniCssExtractPlugin({
        filename: "../css/main.css", 
      }),
      new webpack.ProvidePlugin({
        $: "jquery",
        jQuery: "jquery",
        "window.jQuery": "jquery"
      }),
    ],
    module: {
      rules: [
        {
          test: /\.css$/i,
          use: [
            MiniCssExtractPlugin.loader,
            "css-loader"
          ],
        },
      ],
    },
  };
};
