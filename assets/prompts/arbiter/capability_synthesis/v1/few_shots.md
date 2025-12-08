Example: group issues by label, count, track last updated
```
(capability "example/transform_group-issues-by-label"
  :name "Group issues by label"
  :description "Group issues by label, count, and latest updated_at"
  :version "1.0.0"
  :language "rtfs20"
  :permissions [:data_processing]
  :effects [:pure]
  :input-schema [:map [:data [:vector [:map [:labels [:vector :string]] [:updated_at :string]]]]]
  :output-schema [:map [:default [:map [:count :int] [:last_updated :string]]]]
  :metadata {:sample-input "{:data [{:labels [\"bug\"] :updated_at \"2024-01-01\"}]}" :sample-output "{:bug {:count 1 :last_updated \"2024-01-01\"}}"}
  :implementation
    (fn [input]
      (let [items (get input :data [])
            items (if (vector? items) items [])
            extract-labels (fn [issue]
                             (let [lbls (get issue :labels [])]
                               (cond
                                 (vector? lbls) lbls
                                 (nil? lbls) []
                                 :else [lbls])))
            extract-updated (fn [issue]
                               (or (get issue :updated_at)
                                   (get issue :updated)
                                   ""))]
        (reduce
          (fn [acc issue]
            (let [labels (extract-labels issue)
                  updated (extract-updated issue)]
              (reduce
                (fn [m lbl]
                  (let [k (if (map? lbl) (or (get lbl :name) (str lbl)) (str lbl))
                        existing (get m k {:count 0 :last_updated ""})
                        new-count (+ 1 (get existing :count 0))
                        prev-updated (get existing :last_updated "")]
                    (assoc m k {:count new-count
                                :last_updated (if (> (str updated) (str prev-updated))
                                                 updated
                                                 prev-updated)})))
                acc
                labels)))
          {}
          items))))
```







